package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class TextEditorProfileTest {
    @Test
    fun secretTokenProfile_hasPasswordInputType() {
        val profile = TextEditorProfile.SecretToken
        assertEquals(TextInputType.PASSWORD, profile.inputType)
    }

    @Test
    fun secretTokenProfile_hasMaskAndClearPolicy() {
        val profile = TextEditorProfile.SecretToken
        assertEquals(SecretPolicy.MASK_AND_CLEAR_ON_COMMIT, profile.secretPolicy)
    }

    @Test
    fun secretTokenProfile_blocksCopy() {
        val profile = TextEditorProfile.SecretToken
        assertEquals(CopyPolicy.BLOCK, profile.copyPolicy)
    }

    @Test
    fun secretTokenProfile_allowsPaste() {
        val profile = TextEditorProfile.SecretToken
        assertEquals(PastePolicy.ALLOW, profile.pastePolicy)
    }

    @Test
    fun secretTokenProfile_cursorOnlySelection() {
        val profile = TextEditorProfile.SecretToken
        assertEquals(SelectionPolicy.CURSOR_ONLY, profile.selectionPolicy)
    }

    @Test
    fun repositoryUrlProfile_hasDisabledAutocorrect() {
        val profile = TextEditorProfile.RepositoryUrl
        assertEquals(AutocorrectPolicy.DISABLED, profile.autocorrectPolicy)
    }

    @Test
    fun branchNameProfile_hasDisabledAutocorrect() {
        val profile = TextEditorProfile.BranchName
        assertEquals(AutocorrectPolicy.DISABLED, profile.autocorrectPolicy)
    }

    @Test
    fun replaceQueryProfile_hasDisabledAutocorrect() {
        val profile = TextEditorProfile.ReplaceQuery
        assertEquals(AutocorrectPolicy.DISABLED, profile.autocorrectPolicy)
    }

    @Test
    fun searchQueryProfile_hasSearchImeAction() {
        val profile = TextEditorProfile.SearchQuery
        assertEquals(ImeAction.SEARCH, profile.imeAction)
    }

    @Test
    fun allProfiles_haveSystemSuppressedOrInheritAnimation() {
        val profiles =
            listOf(
                TextEditorProfile.DocumentBody,
                TextEditorProfile.ShortTitle,
                TextEditorProfile.ShortDescription,
                TextEditorProfile.InlineLabel,
                TextEditorProfile.CanvasLabel,
                TextEditorProfile.SearchQuery,
                TextEditorProfile.LongNote,
                TextEditorProfile.SecretToken,
                TextEditorProfile.RepositoryUrl,
                TextEditorProfile.BranchName,
                TextEditorProfile.ReplaceQuery,
            )
        for (profile in profiles) {
            assertTrue(
                "Profile should have INHERIT_GLOBAL or SYSTEM_SUPPRESSED animation, got ${profile.animationPolicy}",
                profile.animationPolicy == AnimationPolicy.INHERIT_GLOBAL ||
                    profile.animationPolicy == AnimationPolicy.SYSTEM_SUPPRESSED,
            )
        }
    }

    @Test
    fun passwordInputType_exists() {
        assertTrue(TextInputType.values().contains(TextInputType.PASSWORD))
    }

    @Test
    fun autocorrectPolicy_hasDisabledOption() {
        assertTrue(AutocorrectPolicy.values().contains(AutocorrectPolicy.DISABLED))
    }

    @Test
    fun capitalizationPolicy_hasAllOptions() {
        assertEquals(4, CapitalizationPolicy.values().size)
        assertTrue(CapitalizationPolicy.values().contains(CapitalizationPolicy.NONE))
        assertTrue(CapitalizationPolicy.values().contains(CapitalizationPolicy.CHARACTERS))
        assertTrue(CapitalizationPolicy.values().contains(CapitalizationPolicy.WORDS))
        assertTrue(CapitalizationPolicy.values().contains(CapitalizationPolicy.SENTENCES))
    }

    @Test
    fun copyPolicy_hasBlockOption() {
        assertTrue(CopyPolicy.values().contains(CopyPolicy.BLOCK))
    }

    @Test
    fun pastePolicy_hasBlockOption() {
        assertTrue(PastePolicy.values().contains(PastePolicy.BLOCK))
    }

    @Test
    fun secretPolicy_hasMaskAndClearOption() {
        assertTrue(SecretPolicy.values().contains(SecretPolicy.MASK_AND_CLEAR_ON_COMMIT))
    }

    @Test
    fun defaultProfile_hasNoneSecretPolicy() {
        val profile = TextEditorProfile()
        assertEquals(SecretPolicy.NONE, profile.secretPolicy)
    }

    @Test
    fun defaultProfile_hasDefaultAutocorrect() {
        val profile = TextEditorProfile()
        assertEquals(AutocorrectPolicy.DEFAULT, profile.autocorrectPolicy)
    }

    @Test
    fun defaultProfile_hasNoneCapitalization() {
        val profile = TextEditorProfile()
        assertEquals(CapitalizationPolicy.NONE, profile.capitalizationPolicy)
    }

    @Test
    fun defaultProfile_hasAllowCopyAndPaste() {
        val profile = TextEditorProfile()
        assertEquals(CopyPolicy.ALLOW, profile.copyPolicy)
        assertEquals(PastePolicy.ALLOW, profile.pastePolicy)
    }
}
