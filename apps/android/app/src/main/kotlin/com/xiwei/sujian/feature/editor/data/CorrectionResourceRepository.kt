package com.xiwei.sujian.feature.editor.data

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import android.util.AtomicFile
import androidx.core.content.edit
import com.xiwei.sujian.core.platform.storage.AndroidPrivateDataRoot
import java.io.File
import java.io.IOException
import java.util.UUID

data class ImportedCorrectionModel(
    val file: File,
    val displayName: String,
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
    private val dictionaryDirectory = File(appDataRoot, DICTIONARY_DIRECTORY)
    private val preferences = appContext.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)

    fun currentFiles(): CorrectionResourceFiles {
        val modelName = preferences.getString(KEY_ACTIVE_MODEL, null)
            ?.takeIf { File(modelDirectory, it).isFile }
        val displayName = modelName?.let { preferences.getString(KEY_ACTIVE_MODEL_DISPLAY_NAME, null) }
        val dictionaries = SUPPORTED_DICTIONARIES.filter { File(dictionaryDirectory, it).isFile }
        return CorrectionResourceFiles(modelName, displayName, dictionaries)
    }

    @Throws(IOException::class)
    fun copyModel(uri: Uri): ImportedCorrectionModel {
        val displayName = queryDisplayName(uri)
        if (!displayName.endsWith(".klm", ignoreCase = true)) {
            throw IOException("Choose a .klm language model")
        }
        if (!modelDirectory.isDirectory && !modelDirectory.mkdirs()) {
            throw IOException("Unable to create the private correction model directory")
        }

        val safeName = "model-${UUID.randomUUID()}.klm"
        val target = File(modelDirectory, safeName)
        copyAtomically(uri, target)
        if (!target.isFile || target.length() == 0L) {
            target.delete()
            throw IOException("The selected language model is empty")
        }
        return ImportedCorrectionModel(target, displayName)
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

    fun discardModel(model: ImportedCorrectionModel) {
        if (model.file.parentFile == modelDirectory &&
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

    companion object {
        private const val PREFERENCES = "correction_resources"
        private const val KEY_ACTIVE_MODEL = "active_model_file"
        private const val KEY_ACTIVE_MODEL_DISPLAY_NAME = "active_model_display_name"
        private const val MODEL_DIRECTORY = "correction-models"
        private const val DICTIONARY_DIRECTORY = "correction-data"

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
