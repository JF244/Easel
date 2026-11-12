package com.truckmirror;

import android.content.Context;
import android.hardware.display.DisplayManager;
import android.hardware.display.VirtualDisplay;
import android.media.projection.MediaProjection;
import android.os.Handler;
import android.os.Looper;
import android.util.DisplayMetrics;
import android.view.Display;
import android.view.Surface;

/**
 * The one screen capture shared by the phone side (which starts it) and the
 * truck side (which shows it). Everything here runs on the main thread.
 */
public final class Mirror {
    public interface Listener {
        void onMirrorChanged();
    }

    /** Longest side of the captured picture; the truck screen is smaller than this. */
    private static final int MAX_SIDE = 1600;

    private static final Handler MAIN = new Handler(Looper.getMainLooper());

    private static MediaProjection projection;
    private static VirtualDisplay display;
    private static Surface surface;
    private static Listener listener;
    private static Runnable onStopped;

    private static int width, height, dpi;
    private static int phoneWidth, phoneHeight;

    private Mirror() {}

    public static boolean isCapturing() {
        return display != null;
    }

    public static int width() { return width; }
    public static int height() { return height; }
    public static int phoneWidth() { return phoneWidth; }
    public static int phoneHeight() { return phoneHeight; }

    public static void setListener(Listener l) {
        listener = l;
    }

    static void start(Context context, MediaProjection p, Runnable stopped) {
        stop();
        projection = p;
        onStopped = stopped;
        measure(context);
        p.registerCallback(new MediaProjection.Callback() {
            @Override
            public void onStop() {
                if (projection == p) release();
            }

            @Override
            public void onCapturedContentResize(int w, int h) {
                if (projection == p) refreshSize(context);
            }
        }, MAIN);
        // Android 14 allows one virtual display per permission grant, so it is
        // made once here and the truck's surface is swapped in and out of it.
        display = p.createVirtualDisplay("TruckMirror", width, height, dpi,
                DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR, surface, null, MAIN);
        notifyListener();
    }

    static void stop() {
        MediaProjection p = projection;
        release();
        if (p != null) {
            try {
                p.stop();
            } catch (RuntimeException ignored) {
            }
        }
    }

    private static void release() {
        if (projection == null && display == null) return;
        projection = null;
        if (display != null) {
            try {
                display.release();
            } catch (RuntimeException ignored) {
            }
            display = null;
        }
        Runnable r = onStopped;
        onStopped = null;
        if (r != null) r.run();
        notifyListener();
    }

    /** The truck side hands over the surface to draw on, or null when it goes away. */
    public static void setSurface(Surface s) {
        surface = s;
        if (display != null) {
            try {
                display.setSurface(s);
            } catch (RuntimeException ignored) {
            }
        }
    }

    /** Called when the phone rotates: the captured picture changes shape with it. */
    static void refreshSize(Context context) {
        int oldW = width, oldH = height;
        measure(context);
        if (width == oldW && height == oldH) return;
        if (display != null) {
            try {
                display.resize(width, height, dpi);
            } catch (RuntimeException ignored) {
            }
        }
        notifyListener();
    }

    private static void measure(Context context) {
        DisplayManager dm = (DisplayManager) context.getSystemService(Context.DISPLAY_SERVICE);
        Display d = dm.getDisplay(Display.DEFAULT_DISPLAY);
        DisplayMetrics m = new DisplayMetrics();
        d.getRealMetrics(m);
        phoneWidth = m.widthPixels;
        phoneHeight = m.heightPixels;
        float scale = Math.min(1f, (float) MAX_SIDE / Math.max(phoneWidth, phoneHeight));
        width = Math.max(2, Math.round(phoneWidth * scale) & ~1);
        height = Math.max(2, Math.round(phoneHeight * scale) & ~1);
        dpi = Math.max(120, Math.round(m.densityDpi * scale));
    }

    private static void notifyListener() {
        if (listener != null) listener.onMirrorChanged();
    }
}
