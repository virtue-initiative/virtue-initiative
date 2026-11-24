package org.virtueinitiative.virtue

import java.io.File
import javax.xml.parsers.DocumentBuilderFactory
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element

// CLIENT-035: Android buttons, menu items, dialog titles and screen titles use sentence case.
class CapitalizationTest {
    /** Words that keep their capital letter mid-label: the app and OS setting names. */
    private val properNouns = setOf("Virtue", "Accessibility", "Android", "OK")

    private fun labels(): Map<String, String> {
        val file = File("src/main/res/values/strings.xml")
        val nodes = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(file)
            .getElementsByTagName("string")
        return (0 until nodes.length).map { nodes.item(it) as Element }
            .associate { it.getAttribute("name") to it.textContent }
            .filterKeys { name ->
                name != "app_title" &&
                    (name.startsWith("btn_") || name.startsWith("menu_") || name.endsWith("_title"))
            }
    }

    /** The words of [label] that are capitalized but don't start a sentence. */
    private fun titleCasedWords(label: String): List<String> {
        val words = label.split(" ").filter { it.isNotEmpty() }
        return words.filterIndexed { i, word ->
            val startsSentence = i == 0 || words[i - 1].last() in ".?!"
            !startsSentence && word.first().isUpperCase() &&
                word.trimEnd('.', '?', '!', '…') !in properNouns
        }
    }

    @Test
    fun findsTheLabels() {
        val labels = labels()
        assertTrue(labels.size > 20)
        assertEquals("Take test screenshot", labels["btn_take_test_screenshot"])
    }

    @Test
    fun labelsUseSentenceCase() {
        val offenders = labels().filterValues { titleCasedWords(it).isNotEmpty() }
        assertEquals(emptyMap<String, String>(), offenders)
    }

    @Test
    fun theCheckCatchesTitleCase() {
        assertEquals(listOf("Test", "Screenshot"), titleCasedWords("Take Test Screenshot"))
        assertEquals(emptyList<String>(), titleCasedWords("Don't have an account? Sign up"))
        assertEquals(emptyList<String>(), titleCasedWords("Open Accessibility settings"))
    }
}
