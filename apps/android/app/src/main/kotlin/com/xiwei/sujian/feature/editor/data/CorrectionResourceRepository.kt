package com.xiwei.sujian.feature.editor.data

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import android.util.AtomicFile
import androidx.core.content.edit
import com.xiwei.sujian.core.platform.storage.AndroidPrivateDataRoot
import java.io.DigestInputStream
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
