package com.xiwei.sujian.feature.editor.data

import android.content.Context
import android.content.res.AssetManager
import android.net.Uri
import android.provider.OpenableColumns
import android.util.AtomicFile
import androidx.core.content.edit
import com.xiwei.sujian.core.platform.storage.AndroidPrivateDataRoot
import java.io.File
import java.io.IOException
import java.io.OutputStream
import java.security.DigestInputStream
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
        val modelName = activeModelFile()?.takeIf { it.isFile }?.name
        val displayName = modelName?.let { preferences.getString(KEY_ACTIVE_MODEL_DISPLAY_NAME, null) }
        val dictionaries = SUPPORTED_DICTIONARIES.filter { File(dictionaryDirectory, it).isFile }
        return CorrectionResourceFiles(modelName, displayName, dictionaries)
    }

    /** Returns the persisted active path even if the file is currently missing. */
    fun activeModelFile(): File? =
        preferences.getString(KEY_ACTIVE_MODEL, null)
            ?.takeIf { it.isNotBlank() && File(it).name == it }
            ?.let { File(modelDirectory, it) }

    @Throws(IOException::class)
    fun stageModel(uri: Uri): ImportedCorrectionModel {
        val displayName = queryDisplayName(uri)
        if (!displayName.endsWith(".klm", ignoreCase = true)) {
            throw IOException("Choose a .klm language model")
        }
        if (!modelStagingDirectory.isDirectory && !modelStagingDirectory.mkdirs()) {
            throw IOException("Unable to create the private correction model staging directory")
        }

        val stagingFile = File(modelStagingDirectory, "${UUID.randomUUID()}.part")
        val contentHash = copyAtomicallyWithHash(uri, stagingFile)
        if (!stagingFile.isFile || stagingFile.length() == 0L) {
            stagingFile.delete()
            throw IOException("The selected language model is empty")
        }

        try {
            return synchronized(modelImportLock) {
                val duplicate = findModelWithHash(contentHash, stagingFile.length())
                if (duplicate != null) {
                    ImportedCorrectionModel(duplicate, displayName, contentHash, createdByImport = false)
                } else {
                    val hashedPath = File(modelDirectory, "model-$contentHash.klm")
                    val target =
                        if (hashedPath.exists()) {
                            File(modelDirectory, "model-$contentHash-${UUID.randomUUID()}.klm")
                        } else {
                            hashedPath
                        }
                    if (!stagingFile.renameTo(target)) {
                        throw IOException("Unable to install the staged correction model")
                    }
                    ImportedCorrectionModel(target, displayName, contentHash, createdByImport = true)
                }
            }
        } finally {
            stagingFile.delete()
        }
    }

    fun setActiveModel(model: ImportedCorrectionModel) {
        require(model.file.parentFile == modelDirectory && model.file.isFile) {
            "The configured model must be in the private correction model directory"
        }
        preferences.edit()
            .putString(KEY_ACTIVE_MODEL, model.file.name)
            .putString(KEY_ACTIVE_MODEL_DISPLAY_NAME, model.displayName)
            .apply()
    }

    /**
     * #958：确保随 APK 打包的内置中文 KenLM 模型已就位并返回其文件。
     *
     * 用户导入的模型始终优先 — 调用方先看 [activeModelFile]；只有没有 active
     * 用户模型、且私有模型目录里也没有摘要一致的副本时，才从 APK assets 流式
     * 拷贝内置模型，成功后按摘要复用（只拷一次）。内置模型不是用户模型，
     * 因此不写 [KEY_ACTIVE_MODEL] 偏好、不调用 [setActiveModel]。
     */
    @Throws(IOException::class)
    fun ensureBundledModel(): File {
        synchronized(modelImportLock) {
            findModelWithHash(BUNDLED_MODEL_SHA256, BUNDLED_MODEL_SIZE)?.let { return it }
            val namedCopy = File(modelDirectory, BUNDLED_MODEL_FILE_NAME)
            if (namedCopy.isFile && namedCopy.length() == BUNDLED_MODEL_SIZE &&
                hashFile(namedCopy) == BUNDLED_MODEL_SHA256
            ) {
                return namedCopy
            }
            if (!modelDirectory.isDirectory && !modelDirectory.mkdirs()) {
                throw IOException("Unable to create the private correction model directory")
            }
            if (!modelStagingDirectory.isDirectory && !modelStagingDirectory.mkdirs()) {
                throw IOException("Unable to create the private correction model staging directory")
            }
            val stagingFile = File(modelStagingDirectory, "${UUID.randomUUID()}.part")
            try {
                copyBundledModelToStaging(stagingFile)
                if (namedCopy.exists() && !namedCopy.delete()) {
                    throw IOException("Unable to replace the existing bundled correction model")
                }
                if (!stagingFile.renameTo(namedCopy)) {
                    throw IOException("Unable to install the bundled correction model")
                }
                return namedCopy
            } finally {
                stagingFile.delete()
            }
        }
    }

    /** Lists installed model copies, excluding temporary candidates under the staging directory. */
    fun installedModels(): List<File> =
        modelDirectory.listFiles()
            ?.filter { it.isFile && it.extension.equals("klm", ignoreCase = true) }
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
        // #958：内置模型是兜底资源，不是用户导入被取代的旧副本 — 用户改选其他模型后
        // 删掉它只会让下次启动重新从 assets 拷贝 148MB。
        if (canonicalFile.name == BUNDLED_MODEL_FILE_NAME) return false
        if (canonicalFile.parentFile != canonicalModelDirectory ||
            !canonicalFile.extension.equals("klm", ignoreCase = true) ||
            preferences.getString(KEY_ACTIVE_MODEL, null) == canonicalFile.name
        ) {
            return false
        }
        return canonicalFile.delete()
    }

    fun discardModel(model: ImportedCorrectionModel) {
        if (model.createdByImport && model.file.parentFile == modelDirectory &&
            preferences.getString(KEY_ACTIVE_MODEL, null) != model.file.name
        ) {
            model.file.delete()
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

    private fun copyAtomicallyWithHash(uri: Uri, target: File): String {
        val input = appContext.contentResolver.openInputStream(uri)
            ?: throw IOException("Unable to open the selected file")
        val atomicFile = AtomicFile(target)
        val digest = MessageDigest.getInstance(SHA_256)
        var output: java.io.FileOutputStream? = null
        try {
            input.use { source ->
                val stream = atomicFile.startWrite()
                output = stream
                DigestInputStream(source, digest).use { it.copyTo(stream) }
                atomicFile.finishWrite(stream)
                output = null
            }
        } catch (error: Exception) {
            output?.let(atomicFile::failWrite)
            target.delete()
            if (error is IOException) throw error
            throw IOException("Unable to stage the selected correction model", error)
        }
        return digest.digest().toLowerHex()
    }

    /**
     * #958：把内置模型从 APK assets 流式拷到临时文件，边写边累计字节数与 SHA-256；
     * 长度或摘要不符时抛 IOException，由 [ensureBundledModel] 删除临时文件。
     * 148MB 模型不能整块读进内存，因此不使用 readBytes()/copyTo()。
     */
    private fun copyBundledModelToStaging(stagingFile: File) {
        val digest = MessageDigest.getInstance(SHA_256)
        val copiedBytes = stagingFile.outputStream().use { output -> streamBundledModel(output, digest) }
        if (copiedBytes != BUNDLED_MODEL_SIZE) {
            throw IOException("The bundled correction model has an unexpected byte count")
        }
        if (digest.digest().toLowerHex() != BUNDLED_MODEL_SHA256) {
            throw IOException("The bundled correction model failed its checksum")
        }
    }

    /** #958：assets → 输出流的固定缓冲拷贝循环；超出期望字节数立即失败，避免无界写入。 */
    private fun streamBundledModel(
        output: OutputStream,
        digest: MessageDigest,
    ): Long {
        var copiedBytes = 0L
        appContext.assets.open(BUNDLED_MODEL_ASSET_PATH, AssetManager.ACCESS_STREAMING).use { input ->
            val buffer = ByteArray(BUNDLED_MODEL_BUFFER_SIZE)
            while (true) {
                val count = input.read(buffer)
                if (count < 0) break
                if (count == 0) continue
                copiedBytes += count
                if (copiedBytes > BUNDLED_MODEL_SIZE) {
                    throw IOException("The bundled correction model exceeds the expected size")
                }
                digest.update(buffer, 0, count)
                output.write(buffer, 0, count)
            }
        }
        return copiedBytes
    }

    private fun findModelWithHash(
        contentHash: String,
        expectedLength: Long,
    ): File? =
        modelDirectory.listFiles()
            ?.asSequence()
            ?.filter { it.isFile && it.extension.equals("klm", ignoreCase = true) && it.length() == expectedLength }
            ?.firstOrNull { hashFile(it) == contentHash }

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
        private val modelImportLock = Any()

        // #958：随 APK 打包的内置中文 KenLM 模型，与
        // packaging/correction/kenlm-model.lock 及 app/build.gradle.kts 中的
        // fetchBundledCorrectionModel 保持一致（文件名、assets 路径、字节数、摘要）。
        internal const val BUNDLED_MODEL_FILE_NAME = "people2014corpus_chars.klm"
        internal const val BUNDLED_MODEL_ASSET_PATH = "correction/people2014corpus_chars.klm"
        internal const val BUNDLED_MODEL_SIZE = 147_608_010L
        internal const val BUNDLED_MODEL_SHA256 =
            "1d4ad3975144619a5bddc4ac53fcf319ec27d37dccb4ba58674af3a068d835a9"
        private const val BUNDLED_MODEL_BUFFER_SIZE = 1024 * 1024

        val SUPPORTED_DICTIONARIES = listOf(
            "word_freq.txt",
            "same_pinyin.txt",
            "same_stroke.txt",
            "custom_confusion.txt",
            "person_name.txt",
            "place_name.txt",
            "proper_name.txt",
            "ignored_words.txt",
        )
    }
}
