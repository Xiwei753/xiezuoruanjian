package com.xiwei.sujian.test.testime;

import android.app.Service;
import android.content.Intent;
import android.os.IBinder;
import android.util.Log;

/**
 * Command relay for the deterministic test IME (issue #589).
 *
 * Why this component exists: the test IME service {@link TestInputMethodService}
 * MUST be protected with {@code android.permission.BIND_INPUT_METHOD}, otherwise
 * InputMethodManagerService does not recognize it as an input method at all. But
 * ActivityManagerService rejects startService/bindService from any non-system
 * caller for a service with that protection level ("Error: Requires permission
 * android.permission.BIND_INPUT_METHOD", verified on API 34), so the test
 * process (app uid) can never start the IME component directly.
 *
 * This relay lives in the same test APK (and therefore the same process,
 * com.xiwei.sujian.test) and declares NO permission, so the app process CAN
 * start it with an explicit component Intent. It forwards the untouched command
 * Intent to the currently bound {@link TestInputMethodService} instance
 * (registered via {@link TestInputMethodService#getActiveInstance()}); the IME
 * then executes the command on its current InputConnection — the exact
 * InputMethodService -> InputConnection -> adapter -> kernel -> UI path a real
 * IME uses. Both services run on the same main thread, so the relay's
 * onStartCommand and the IME's connection handling are naturally ordered.
 *
 * All command logging still happens under the IME's tag "SujianTestIme".
 *
 * IMPORTANT: like the IME, this class is intentionally written in plain Java —
 * the test APK does not bundle kotlin-stdlib and this service runs in the test
 * APK's own process.
 */
public class TestImeCommandService extends Service {

    public static final String TAG = TestInputMethodService.TAG;

    /** Component id used by TestImeCommands.kt and by the workflow if needed. */
    public static final String COMPONENT_ID =
            "com.xiwei.sujian.test/.testime.TestImeCommandService";

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent == null) {
            Log.w(TAG, "relay onStartCommand: null intent (system restart), startId=" + startId);
            return START_NOT_STICKY;
        }
        String command = intent.getStringExtra(TestInputMethodService.EXTRA_COMMAND);
        if (command == null) {
            Log.w(TAG, "relay onStartCommand: missing command extra (action="
                    + intent.getAction() + "), startId=" + startId);
            return START_NOT_STICKY;
        }
        TestInputMethodService ime = TestInputMethodService.getActiveInstance();
        if (ime == null) {
            Log.w(TAG, "relay onStartCommand: command=" + command
                    + " DROPPED (no active TestInputMethodService — the IME has not been "
                    + "bound to an editor input session yet), startId=" + startId);
            return START_NOT_STICKY;
        }
        Log.i(TAG, "relay onStartCommand: forwarding command=" + command
                + " to IME, startId=" + startId);
        return ime.executeCommand(intent);
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}
