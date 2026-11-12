package com.truckmirror;

import android.app.Activity;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.hardware.display.DisplayManager;
import android.media.projection.MediaProjection;
import android.media.projection.MediaProjectionManager;
import android.os.Build;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.view.Display;

/** Keeps the screen capture alive while the app is not on screen. */
public class CaptureService extends Service {
    static final String ACTION_START = "com.truckmirror.START";
    static final String ACTION_STOP = "com.truckmirror.STOP";
    static final String EXTRA_CODE = "code";
    static final String EXTRA_DATA = "data";

    private static final String CHANNEL = "capture";

    private final DisplayManager.DisplayListener displayListener = new DisplayManager.DisplayListener() {
        @Override
        public void onDisplayAdded(int id) {}

        @Override
        public void onDisplayRemoved(int id) {}

        @Override
        public void onDisplayChanged(int id) {
            if (id == Display.DEFAULT_DISPLAY) Mirror.refreshSize(CaptureService.this);
        }
    };

    static Intent startIntent(Context context, int resultCode, Intent data) {
        return new Intent(context, CaptureService.class)
                .setAction(ACTION_START)
                .putExtra(EXTRA_CODE, resultCode)
                .putExtra(EXTRA_DATA, data);
    }

    static Intent stopIntent(Context context) {
        return new Intent(context, CaptureService.class).setAction(ACTION_STOP);
    }

    @Override
    public void onCreate() {
        super.onCreate();
        DisplayManager dm = (DisplayManager) getSystemService(DISPLAY_SERVICE);
        dm.registerDisplayListener(displayListener, new Handler(Looper.getMainLooper()));
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent == null || !ACTION_START.equals(intent.getAction())) {
            Mirror.stop();
            stopSelf();
            return START_NOT_STICKY;
        }

        // Must be in the foreground, as a screen-capture service, before the
        // capture permission is turned into a MediaProjection.
        Notification n = notification();
        if (Build.VERSION.SDK_INT >= 29) {
            startForeground(1, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION);
        } else {
            startForeground(1, n);
        }

        int code = intent.getIntExtra(EXTRA_CODE, Activity.RESULT_CANCELED);
        Intent data = intent.getParcelableExtra(EXTRA_DATA);
        MediaProjectionManager mpm =
                (MediaProjectionManager) getSystemService(MEDIA_PROJECTION_SERVICE);
        MediaProjection projection = data == null ? null : mpm.getMediaProjection(code, data);
        if (projection == null) {
            stopSelf();
            return START_NOT_STICKY;
        }
        Mirror.start(this, projection, this::stopSelf);
        return START_NOT_STICKY;
    }

    @Override
    public void onDestroy() {
        DisplayManager dm = (DisplayManager) getSystemService(DISPLAY_SERVICE);
        dm.unregisterDisplayListener(displayListener);
        Mirror.stop();
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    private Notification notification() {
        NotificationManager nm = getSystemService(NotificationManager.class);
        nm.createNotificationChannel(new NotificationChannel(
                CHANNEL, "Screen sharing", NotificationManager.IMPORTANCE_LOW));
        PendingIntent open = PendingIntent.getActivity(this, 0,
                new Intent(this, MainActivity.class), PendingIntent.FLAG_IMMUTABLE);
        PendingIntent stop = PendingIntent.getService(this, 1,
                stopIntent(this), PendingIntent.FLAG_IMMUTABLE);
        return new Notification.Builder(this, CHANNEL)
                .setSmallIcon(android.R.drawable.ic_menu_slideshow)
                .setContentTitle("Truck Mirror is sharing your screen")
                .setContentText("Open Truck Mirror on the truck display to see it.")
                .setContentIntent(open)
                .addAction(new Notification.Action.Builder(null, "Stop", stop).build())
                .setOngoing(true)
                .build();
    }
}
