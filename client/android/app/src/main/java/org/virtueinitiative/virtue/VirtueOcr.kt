package org.virtueinitiative.virtue

import android.graphics.BitmapFactory
import com.google.android.gms.tasks.Tasks
import com.google.mlkit.vision.common.InputImage
import com.google.mlkit.vision.text.TextRecognition
import com.google.mlkit.vision.text.latin.TextRecognizerOptions

object VirtueOcr {
    @JvmStatic
    fun recognizeText(imageBytes: ByteArray, language: String?): String {
        val bitmap = BitmapFactory.decodeByteArray(imageBytes, 0, imageBytes.size) ?: return ""
        val image = InputImage.fromBitmap(bitmap, 0)
        val recognizer = TextRecognition.getClient(TextRecognizerOptions.DEFAULT_OPTIONS)
        return try {
            val result = Tasks.await(recognizer.process(image))
            result.textBlocks.flatMap { block ->
                block.lines.flatMap { line ->
                    line.elements.mapNotNull { element ->
                        val box = element.boundingBox ?: return@mapNotNull null
                        "${element.text}|${box.left}|${box.top}|${box.right}|${box.bottom}"
                    }
                }
            }.joinToString("\n")
        } catch (e: Exception) {
            ""
        } finally {
            recognizer.close()
        }
    }
}
