package com.xiwei.sujian.feature.editor.data

import android.content.Context
import android.content.res.AssetManager
import android.net.Uri
import android.provider.OpenableColumns
import android.util.AtomicFile
import androidx.core.content.edit
import androidx.documentfile.provider.DocumentFile
import com.xiwei.sujian.core.platform.storage.AndroidPrivateDataRoot
import java.io.File
import java.io.IOException
import java.security.MessageDigest
import java.util.UUID

data class ImportedCorrectionModel(
    val file: File,
    val displayName: String,
    val contentHash: String,
    val createdByImport: Boolean,
)

data class CorrectionResourceFiles(
    val activeModelName: String?,
    val activeModelDisplayName: String?,
    val dictionaryFiles: List<String>,
)

data class ImportedCorrectionDictionaries(
    val importedFiles: List<String>,
    val unsupportedFiles: List<String>,
)

/** Owns private copies of user-selected correction resources. */
class CorrectionResourceRepository(context: Context) {
    private val appContext = context.applicationContext
    private val appDataRoot = AndroidPrivateDataRoot.root(appContext)
    private val modelDirectory = File(appDataRoot, MODEL_DIRECTORY)
    private val modelStagingDirectory = File(modelDirectory, MODEL_STAGING_DIRECTORY)
    private val dictionaryDirectory = File(appDataRoot, DICTIONARY_DIRECTORY)
    private val preferences = appContext.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)

    fun currentFiles(): CorrectionResourceFiles {
        val modelName = activeModelFile()?.takeIf(::isModelBundle)?.name
        val displayName = modelName?.let { preferences.getString(KEY_ACTIVE_MODEL_DISPLAY_NAME, null) }
        val dictionaries = SUPPORTED_DICTIONARIES.filter { File(dictionaryDirectory, it).isFile }
        return CorrectionResourceFiles(modelName, displayName, dictionaries)
    }

    /** Returns the persisted active path even if the model directory is currently missing. */
    fun activeModelFile(): File? =
        preferences.getString(KEY_ACTIVE_MODEL, null)
            ?.takeIf { it.isNotBlank() && File(it).name == it }
            ?.let { File(modelDirectory, it) }

    /**
     * #985：复制用户通过目录选择器选中的 ONNX 资源包。资源包必须包含 model.onnx
     * 与 vocab.txt；可选 calibrator.json 和 BERT tokenizer 元数据会一并复制。
     */
    @Throws(IOException::class)
    fun stageModel(treeUri: Uri): ImportedCorrectionModel {
        val directory =
            DocumentFile.fromTreeUri(appContext, treeUri)
                ?: throw IOException("Unable to open the selected model directory")
        val displayName = directory.name?.takeIf(String::isNotBlank) ?: "correction-model"
        val modelDocument = directory.findFile(MODEL_FILE_NAME)
        val vocabDocument = directory.findFile(VOCAB_FILE_NAME)
        if (modelDocument?.isFile != true || vocabDocument?.isFile != true) {
            throw IOException("The selected directory must contain model.onnx and vocab.txt")
        }
        if (!modelStagingDirectory.isDirectory && !modelStagingDirectory.mkdirs()) {
            throw IOException("Unable to create the private correction model staging directory")
        }
        if (!modelDirectory.isDirectory && !modelDirectory.mkdirs()) {
            throw IOException("Unable to create the private correction model directory")
        }

        val stagingDirectory = File(modelStagingDirectory, "${UUID.randomUUID()}.part")
        if (!stagingDirectory.mkdirs()) {
            throw IOException("Unable to create the correction model staging directory")
        }
        try {
            for (name in MODEL_BUNDLE_FILES) {
                val document = directory.findFile(name) ?: continue
                if (!document.isFile) continue
                copyDocument(document.uri, File(stagingDirectory, name))
            }
            val stagedModel = File(stagingDirectory, MODEL_FILE_NAME)
            val stagedVocab = File(stagingDirectory, VOCAB_FILE_NAME)
            if (!stagedModel.isFile || stagedModel.length() == 0L || !stagedVocab.isFile) {
                throw IOException("The selected model bundle is incomplete")
            }
            val contentHash = hashFile(stagedModel)

            return synchronized(modelImportLock) {
                val duplicate = findModelWithHash(contentHash)
                if (duplicate != null) {
                    ImportedCorrectionModel(duplicate, displayName, contentHash, createdByImport = false)
                } else {
                    val target =
                        File(
                            modelDirectory,
                            "model-${contentHash.take(HASH_DIRECTORY_CHARS)}",
                        )
                    if (target.exists() && !target.deleteRecursively()) {
                        throw IOException("Unable to replace the staged correction model")
                    }
                    if (!stagingDirectory.renameTo(target)) {
                        throw IOException("Unable to install the staged correction model")
                    }
                    ImportedCorrectionModel(target, displayName, contentHash, createdByImport = true)
                }
            }
        } finally {
            if (stagingDirectory.exists()) stagingDirectory.deleteRecursively()
        }
    }

    fun setActiveModel(model: ImportedCorrectionModel) {
        require(model.file.parentFile == modelDirectory && isModelBundle(model.file)) {
            "The configured model must be a complete bundle in the private correction model directory"
        }
        preferences.edit()
            .putString(KEY_ACTIVE_MODEL, model.file.name)
            .putString(KEY_ACTIVE_MODEL_DISPLAY_NAME, model.displayName)
            .apply()
    }

    /**
     * #985：确保随 APK 打包的 ONNX 纠错模型资源目录已就位并返回目录路径。
     * 模型是约 474MB 的 FP32 权重，不能整块读入内存；按文件逐个流式复制并校验
     * model.onnx 的长度和 SHA-256，成功后按目录复用。
     */
    @Throws(IOException::class)
    fun ensureBundledModel(): File {
        synchronized(modelImportLock) {
            val target = File(modelDirectory, BUNDLED_MODEL_DIRECTORY_NAME)
            if (isModelBundle(target)) {
                val model = File(target, MODEL_FILE_NAME)
                if (model.length() == BUNDLED_MODEL_SIZE && hashFile(model) == BUNDLED_MODEL_SHA256) {
                    return target
                }
            }
            if (!modelDirectory.isDirectory && !modelDirectory.mkdirs()) {
                throw IOException("Unable to create the private correction model directory")
            }
            if (!modelStagingDirectory.isDirectory && !modelStagingDirectory.mkdirs()) {
                throw IOException("Unable to create the private correction model staging directory")
            }
            val stagingDirectory = File(modelStagingDirectory, "${UUID.randomUUID()}.part")
            if (!stagingDirectory.mkdirs()) {
                throw IOException("Unable to create the bundled model staging directory")
            }
            try {
                copyBundledBundle(stagingDirectory)
                if (target.exists() && !target.deleteRecursively()) {
                    throw IOException("Unable to replace the existing bundled correction model")
                }
                if (!stagingDirectory.renameTo(target)) {
                    throw IOException("Unable to install the bundled correction model")
                }
                return target
            } finally {
                if (stagingDirectory.exists()) stagingDirectory.deleteRecursively()
            }
        }
    }

    /** Lists installed model bundles, excluding temporary candidates under the staging directory. */
    fun installedModels(): List<File> =
        modelDirectory.listFiles()
            ?.filter { it.isDirectory && it.name != MODEL_STAGING_DIRECTORY && isModelBundle(it) }
            .orEmpty()

    /**
     * Deletes a prior model only after Core confirms that it is neither configured nor leased by
     * an in-flight correction operation. The caller supplies the path-specific Core status.
     */
    fun deleteModelIfCoreReleased(
        file: File,
        configured: Boolean,
        inUse: Boolean,
        deletionEligible: Boolean,
    ): Boolean {
        if (configured || inUse || !deletionEligible) return false
        val canonicalModelDirectory = modelDirectory.canonicalFile
        val canonicalFile = file.canonicalFile
        // #985：内置模型是兜底资源，不是用户导入被取代的旧副本。
        if (canonicalFile.name == BUNDLED_MODEL_DIRECTORY_NAME) return false
        if (canonicalFile.parentFile != canonicalModelDirectory ||
            !canonicalFile.isDirectory ||
            preferences.getString(KEY_ACTIVE_MODEL, null) == canonicalFile.name
        ) {
            return false
        }
        return canonicalFile.deleteRecursively()
    }

    fun discardModel(model: ImportedCorrectionModel) {
        if (model.createdByImport && model.file.parentFile == modelDirectory &&
            model.file.name != BUNDLED_MODEL_DIRECTORY_NAME &&
            preferences.getString(KEY_ACTIVE_MODEL, null) != model.file.name
        ) {
            model.file.deleteRecursively()
        }
    }

    @Throws(IOException::class)
    fun importDictionaries(uris: List<Uri>): ImportedCorrectionDictionaries {
        if (!dictionaryDirectory.isDirectory && !dictionaryDirectory.mkdirs()) {
            throw IOException("Unable to create the private correction dictionary directory")
        }

        val imported = mutableListOf<String>()
        val unsupported = mutableListOf<String>()
        for (uri in uris) {
            val displayName = queryDisplayName(uri)
            if (displayName !in SUPPORTED_DICTIONARIES) {
                unsupported += displayName
                continue
            }
            copyAtomically(uri, File(dictionaryDirectory, displayName))
            imported += displayName
        }
        return ImportedCorrectionDictionaries(imported, unsupported)
    }

    private fun queryDisplayName(uri: Uri): String {
        val cursor =
            appContext.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
        cursor?.use {
            if (it.moveToFirst()) {
                val column = it.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                if (column >= 0) {
                    it.getString(column)?.takeIf(String::isNotBlank)?.let { name -> return name }
                }
            }
        }
        return uri.lastPathSegment?.substringAfterLast('/')?.takeIf(String::isNotBlank)
            ?: throw IOException("The selected file has no display name")
    }

    private fun copyAtomically(uri: Uri, target: File) {
        val input = appContext.contentResolver.openInputStream(uri)
            ?: throw IOException("Unable to open the selected file")
        val atomicFile = AtomicFile(target)
        var output: java.io.FileOutputStream? = null
        try {
            input.use { source ->
                val stream = atomicFile.startWrite()
                output = stream
                source.copyTo(stream)
                atomicFile.finishWrite(stream)
                output = null
            }
        } catch (error: Exception) {
            output?.let(atomicFile::failWrite)
            if (error is IOException) throw error
            throw IOException("Unable to copy the selected correction resource", error)
        }
    }

    private fun copyDocument(uri: Uri, target: File) {
        val input = appContext.contentResolver.openInputStream(uri)
            ?: throw IOException("Unable to open $target")
        input.use { source ->
            target.outputStream().use { output -> source.copyTo(output) }
        }
    }

    private fun copyBundledBundle(stagingDirectory: File) {
        val assetRoot = "$BUNDLED_MODEL_ASSET_PATH/"
        for (name in MODEL_BUNDLE_FILES) {
            val assetPath = assetRoot + name
            val target = File(stagingDirectory, name)
            appContext.assets.open(assetPath, AssetManager.ACCESS_STREAMING).use { input ->
                target.outputStream().use { output -> input.copyTo(output) }
            }
        }
        val model = File(stagingDirectory, MODEL_FILE_NAME)
        val vocab = File(stagingDirectory, VOCAB_FILE_NAME)
        if (!model.isFile || model.length() != BUNDLED_MODEL_SIZE || hashFile(model) != BUNDLED_MODEL_SHA256) {
            throw IOException("The bundled correction model failed its size or checksum check")
        }
        if (!vocab.isFile || vocab.length() == 0L) {
            throw IOException("The bundled correction vocabulary is missing")
        }
    }

    private fun findModelWithHash(contentHash: String): File? =
        modelDirectory.listFiles()
            ?.asSequence()
            ?.filter { it.isDirectory && it.name != MODEL_STAGING_DIRECTORY && isModelBundle(it) }
            ?.firstOrNull {
                it.name == "model-${contentHash.take(HASH_DIRECTORY_CHARS)}" ||
                    File(it, MODEL_FILE_NAME).let { model -> model.length() > 0L && hashFile(model) == contentHash }
            }

    private fun isModelBundle(directory: File): Boolean =
        directory.isDirectory &&
            File(directory, MODEL_FILE_NAME).isFile &&
            File(directory, VOCAB_FILE_NAME).isFile

    private fun hashFile(file: File): String {
        val digest = MessageDigest.getInstance(SHA_256)
        file.inputStream().buffered().use { input ->
            val buffer = ByteArray(HASH_BUFFER_SIZE)
            while (true) {
                val count = input.read(buffer)
                if (count < 0) break
                if (count > 0) digest.update(buffer, 0, count)
            }
        }
        return digest.digest().toLowerHex()
    }

    private fun ByteArray.toLowerHex(): String =
        joinToString(separator = "") { byte -> "%02x".format(byte.toInt() and 0xff) }

    companion object {
        private const val PREFERENCES = "correction_resources"
        private const val KEY_ACTIVE_MODEL = "active_model_file"
        private const val KEY_ACTIVE_MODEL_DISPLAY_NAME = "active_model_display_name"
        private const val MODEL_DIRECTORY = "correction-models"
        private const val MODEL_STAGING_DIRECTORY = ".staging"
        private const val DICTIONARY_DIRECTORY = "correction-data"
        private const val SHA_256 = "SHA-256"
        private const val HASH_BUFFER_SIZE = 64 * 1024
        private const val HASH_DIRECTORY_CHARS = 24
        private val modelImportLock = Any()

        internal const val MODEL_FILE_NAME = "model.onnx"
        internal const val VOCAB_FILE_NAME = "vocab.txt"

        // #985：随 APK 打包的 MacBERT4CSC ONNX 资源目录，与
        // crates/sujian-proof/assets/model_manifest.json 保持一致。
        internal const val BUNDLED_MODEL_DIRECTORY_NAME = "macbert4csc"
        internal const val BUNDLED_MODEL_ASSET_PATH = "correction/macbert4csc"
        internal const val BUNDLED_MODEL_SIZE = 474_336_933L
        internal const val BUNDLED_MODEL_SHA256 =
            "a4d9d6807c0c8bc9d014462a728e7212315630c669a6bd670b653154391a73f3"

        internal val MODEL_BUNDLE_FILES =
            listOf(
                MODEL_FILE_NAME,
                VOCAB_FILE_NAME,
                "config.json",
                "tokenizer.json",
                "tokenizer_config.json",
                "special_tokens_map.json",
                "calibrator.json",
            )

        val SUPPORTED_DICTIONARIES = listOf(
            "word_freq.txt",
            "custom_confusion.txt",
            "person_name.txt",
            "place_name.txt",
            "proper_name.txt",
            "ignored_words.txt",
        )
    }
}
