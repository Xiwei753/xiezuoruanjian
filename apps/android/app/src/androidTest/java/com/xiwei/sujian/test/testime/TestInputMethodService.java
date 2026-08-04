package com.xiwei.sujian.test.testime;

import android.content.Intent;
import android.inputmethodservice.InputMethodService;
import android.util.Log;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;

/**
 * Deterministic test InputMethodService for the emulator instrumented tests
 * (issue #589).
 *
 * Lives in the androidTest source set, so it ships only inside the test APK
 * (com.xiwei.sujian.test) and never inside a production APK. The class package
 * MUST be {@code com.xiwei.sujian.test.testime}: the androidTest
 * AndroidManifest declares it with the relative name
 * {@code .testime.TestInputMethodService}, which resolves against the test APK
 * package {@code com.xiwei.sujian.test}, matching the component id used by
 * {@code adb shell ime enable/set} and by the explicit startService command
 * channel: {@code com.xiwei.sujian.test/.testime.TestInputMethodService}.
 *
 * This class is deliberately written in JAVA (the only Kotlin-free class in
 * the app module): the IME runs in the test APK's OWN process, where only the
 * test APK's dex is loaded. The androidTest APK does not package
 * kotlin-stdlib (instrumented tests normally execute inside the app process,
 * which provides the stdlib from the app APK), so any Kotlin class touched by
 * the IME process crashes with NoClassDefFoundError on
 * kotlin.jvm.internal.Intrinsics. Java keeps the service self-contained in
 * the test APK with framework APIs only.
 *
 * The service is completely passive by itself: it shows no input view, never
 * echoes text, never starts composition on its own and never reacts to
 * onUpdateSelection (no recorrection). The only way to make it act is a
 * deterministic command channel: the test process (app process) sends an
 * explicit startService Intent; the command arrives via
 * {@link com.xiwei.sujian.testime.TestImeCommandService} (the IME component
 * itself is BIND_INPUT_METHOD-protected, which blocks startService from any
 * non-system caller) and is executed in
 * {@link #executeCommand(Intent)} against the current InputConnection — the
 * exact InputMethodService -> InputConnection -> adapter -> kernel -> UI path
 * a real IME uses.
 *
 * Supported commands (extras documented in TestImeCommands):
 * COMMIT_TEXT(text, cursor) / SET_COMPOSING_TEXT(text, cursor) /
 * SET_COMPOSING_REGION(start, end) / FINISH_COMPOSING_TEXT /
 * SET_SELECTION(start, end).
 *
 * Every received command and the observed InputConnection state are logged
 * under the {@link #TAG} tag as failure evidence (CI exports logcat on
 * failure).
 */
public class TestInputMethodService extends InputMethodService {

    public static final String TAG = "SujianTestIme";

    /**
     * The currently bound IME instance (same process as the command relay).
     * Registered on onCreate, cleared on onDestroy.
     */
    private static volatile TestInputMethodService activeInstance = null;

    public static TestInputMethodService getActiveInstance() {
        return activeInstance;
    }

    /** Component id used by {@code adb shell ime enable/set} and by the workflow. */
    public static final String COMPONENT_ID =
            "com.xiwei.sujian.test/.testime.TestInputMethodService";

    public static final String EXTRA_COMMAND = "sujian_test_ime_command";

    public static final String COMMAND_COMMIT_TEXT = "commit_text";
    public static final String COMMAND_SET_COMPOSING_TEXT = "set_composing_text";
    public static final String COMMAND_SET_COMPOSING_REGION = "set_composing_region";
    public static final String COMMAND_FINISH_COMPOSING_TEXT = "finish_composing_text";
    public static final String COMMAND_SET_SELECTION = "set_selection";

    public static final String EXTRA_TEXT = "sujian_test_ime_text";
    public static final String EXTRA_CURSOR = "sujian_test_ime_cursor";
    public static final String EXTRA_START = "sujian_test_ime_start";
    public static final String EXTRA_END = "sujian_test_ime_end";

    @Override
    public void onCreate() {
        super.onCreate();
        activeInstance = this;
        Log.i(TAG, "onCreate: registered activeInstance");
    }

    @Override
    public void onDestroy() {
        if (activeInstance == this) {
            activeInstance = null;
        }
        Log.i(TAG, "onDestroy: activeInstance cleared");
        super.onDestroy();
    }

    @Override
    public void onBindInput() {
        super.onBindInput();
        Log.i(TAG, "onBindInput: currentInputConnection=" + (getCurrentInputConnection() != null));
    }

    @Override
    public void onStartInput(EditorInfo attribute, boolean restarting) {
        super.onStartInput(attribute, restarting);
        Log.i(
                TAG,
                "onStartInput: restarting=" + restarting
                        + " inputType=" + (attribute != null ? attribute.inputType : -1)
                        + " imeOptions=" + (attribute != null ? attribute.imeOptions : -1)
                        + " currentInputConnection=" + (getCurrentInputConnection() != null));
    }

    @Override
    public void onFinishInput() {
        Log.i(TAG, "onFinishInput: currentInputConnection=" + (getCurrentInputConnection() != null));
        super.onFinishInput();
    }

    /**
     * Deliberately passive: log the selection change the editor reported back,
     * never react to it (no recorrection, no composing-region echo).
     */
    @Override
    @Deprecated
    public void onUpdateSelection(
            int oldSelStart, int oldSelEnd, int newSelStart, int newSelEnd,
            int candidatesStart, int candidatesEnd) {
        super.onUpdateSelection(
                oldSelStart, oldSelEnd, newSelStart, newSelEnd, candidatesStart, candidatesEnd);
        Log.i(
                TAG,
                "onUpdateSelection(old=" + oldSelStart + ".." + oldSelEnd
                        + " new=" + newSelStart + ".." + newSelEnd
                        + " candidates=" + candidatesStart + ".." + candidatesEnd
                        + "): IGNORED (passive)");
    }

    /**
     * Direct entry point used by {@code TestImeCommandService} (same process).
     * The Android framework cannot startService this IME from the app process:
     * the service must be protected with BIND_INPUT_METHOD for the system to
     * recognize it as an input method, and ActivityManagerService rejects
     * startService from non-system callers for protected services. Commands
     * therefore arrive here via the relay, which forwards the original Intent
     * untouched.
     */
    public int executeCommand(Intent intent) {
        String command = intent.getStringExtra(EXTRA_COMMAND);
        if (command == null) {
            Log.w(TAG, "executeCommand: missing " + EXTRA_COMMAND
                    + " (action=" + intent.getAction() + ")");
            return START_NOT_STICKY;
        }

        InputConnection ic = getCurrentInputConnection();
        CharSequence textBeforeCursor = null;
        if (ic != null) {
            try {
                textBeforeCursor = ic.getTextBeforeCursor(16, 0);
            } catch (RuntimeException e) {
                Log.w(TAG, "executeCommand: getTextBeforeCursor failed: " + e);
            }
        }
        Log.i(
                TAG,
                "executeCommand: command=" + command
                        + " ic=" + (ic != null ? "bound" : "null")
                        + " textBeforeCursor=\""
                        + (textBeforeCursor != null ? textBeforeCursor.toString() : "")
                        + "\"");

        if (ic == null) {
            Log.w(TAG, "executeCommand: command=" + command
                    + " DROPPED (no current InputConnection)");
            return START_NOT_STICKY;
        }

        boolean applied;
        switch (command) {
            case COMMAND_COMMIT_TEXT:
                String commitText = intent.getStringExtra(EXTRA_TEXT);
                if (commitText == null) {
                    commitText = "";
                }
                int commitCursor = intent.getIntExtra(EXTRA_CURSOR, 1);
                applied = ic.commitText(commitText, commitCursor);
                Log.i(TAG, "commitText(text=" + commitText + ", cursor=" + commitCursor
                        + ") -> applied=" + applied);
                break;
            case COMMAND_SET_COMPOSING_TEXT:
                String composingText = intent.getStringExtra(EXTRA_TEXT);
                if (composingText == null) {
                    composingText = "";
                }
                int composingCursor = intent.getIntExtra(EXTRA_CURSOR, 1);
                applied = ic.setComposingText(composingText, composingCursor);
                Log.i(TAG, "setComposingText(text=" + composingText + ", cursor="
                        + composingCursor + ") -> applied=" + applied);
                break;
            case COMMAND_SET_COMPOSING_REGION:
                int regionStart = intent.getIntExtra(EXTRA_START, -1);
                int regionEnd = intent.getIntExtra(EXTRA_END, -1);
                applied = ic.setComposingRegion(regionStart, regionEnd);
                Log.i(TAG, "setComposingRegion(start=" + regionStart + ", end=" + regionEnd
                        + ") -> applied=" + applied);
                break;
            case COMMAND_FINISH_COMPOSING_TEXT:
                applied = ic.finishComposingText();
                Log.i(TAG, "finishComposingText() -> applied=" + applied);
                break;
            case COMMAND_SET_SELECTION:
                int selectionStart = intent.getIntExtra(EXTRA_START, 0);
                int selectionEnd = intent.getIntExtra(EXTRA_END, 0);
                applied = ic.setSelection(selectionStart, selectionEnd);
                Log.i(TAG, "setSelection(start=" + selectionStart + ", end=" + selectionEnd
                        + ") -> applied=" + applied);
                break;
            default:
                Log.w(TAG, "executeCommand: unknown command=" + command);
                break;
        }
        return START_NOT_STICKY;
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent == null) {
            Log.w(TAG, "onStartCommand: null intent (system restart), startId=" + startId);
            return START_NOT_STICKY;
        }
        // The app process cannot startService this component (BIND_INPUT_METHOD
        // protection); commands reach the IME through TestImeCommandService.
        Log.i(TAG, "onStartCommand: direct command (unexpected), startId=" + startId);
        return executeCommand(intent);
    }
}
