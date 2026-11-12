package com.truckmirror.car;

import android.content.Context;
import android.graphics.Color;
import android.graphics.SurfaceTexture;
import android.os.Bundle;
import android.os.PowerManager;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.Surface;
import android.view.TextureView;
import android.view.View;
import android.view.ViewGroup;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.TextView;

import com.google.android.apps.auto.sdk.CarActivity;
import com.google.android.apps.auto.sdk.CarUiController;
import com.truckmirror.Mirror;
import com.truckmirror.TouchService;

/** The truck screen: the phone's picture, plus Back, Home and Recents buttons. */
public class MirrorCarActivity extends CarActivity
        implements Mirror.Listener, TextureView.SurfaceTextureListener {

    private FrameLayout stage;
    private TextureView picture;
    private TextView message;
    private Surface surface;
    private PowerManager.WakeLock wakeLock;

    @Override
    public void onCreate(Bundle state) {
        super.onCreate(state);

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.HORIZONTAL);
        root.setBackgroundColor(Color.BLACK);

        LinearLayout buttons = new LinearLayout(this);
        buttons.setOrientation(LinearLayout.VERTICAL);
        buttons.setBackgroundColor(0xFF111827);
        buttons.addView(button("◀", () -> { TouchService t = touch(); if (t != null) t.back(); }), weighted());
        buttons.addView(button("●", () -> { TouchService t = touch(); if (t != null) t.home(); }), weighted());
        buttons.addView(button("■", () -> { TouchService t = touch(); if (t != null) t.recents(); }), weighted());
        root.addView(buttons, new LinearLayout.LayoutParams(dp(72), ViewGroup.LayoutParams.MATCH_PARENT));

        stage = new FrameLayout(this);
        picture = new TextureView(this);
        picture.setSurfaceTextureListener(this);
        picture.setOnTouchListener(this::onPictureTouch);
        stage.addView(picture, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT, Gravity.CENTER));

        message = new TextView(this);
        message.setTextColor(Color.WHITE);
        message.setTextSize(TypedValue.COMPLEX_UNIT_SP, 22);
        message.setGravity(Gravity.CENTER);
        message.setPadding(dp(24), dp(24), dp(24), dp(24));
        message.setBackgroundColor(Color.BLACK);
        stage.addView(message, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        stage.addOnLayoutChangeListener((v, l, t, r, b, ol, ot, or, ob) -> fitPicture());
        root.addView(stage, new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1));

        setContentView(root);

        try {
            CarUiController ui = getCarUiController();
            ui.getStatusBarController().hideAppHeader();
            ui.getMenuController().hideMenuButton();
        } catch (Throwable ignored) {
            // Cosmetic only: newer Android Auto versions may not offer these.
        }
    }

    @Override
    public void onStart() {
        super.onStart();
        Mirror.setListener(this);
        onMirrorChanged();
        // A phone that locks stops sharing its screen, so keep it awake while
        // the truck is showing it.
        try {
            PowerManager pm = (PowerManager) getSystemService(Context.POWER_SERVICE);
            wakeLock = pm.newWakeLock(
                    PowerManager.SCREEN_DIM_WAKE_LOCK | PowerManager.ACQUIRE_CAUSES_WAKEUP,
                    "truckmirror:car");
            wakeLock.acquire();
        } catch (Throwable ignored) {
        }
    }

    @Override
    public void onStop() {
        Mirror.setListener(null);
        Mirror.setSurface(null);
        if (wakeLock != null) {
            try {
                wakeLock.release();
            } catch (Throwable ignored) {
            }
            wakeLock = null;
        }
        super.onStop();
    }

    @Override
    public void onMirrorChanged() {
        if (Mirror.isCapturing()) {
            message.setVisibility(View.GONE);
            SurfaceTexture st = picture.getSurfaceTexture();
            if (st != null) {
                st.setDefaultBufferSize(Mirror.width(), Mirror.height());
                if (surface == null) surface = new Surface(st);
                Mirror.setSurface(surface);
            }
            fitPicture();
        } else {
            message.setText("Open Truck Mirror on your phone\nand tap “Start screen sharing”.");
            message.setVisibility(View.VISIBLE);
        }
    }

    /** Sizes the picture to the phone's shape, as large as fits, centred. */
    private void fitPicture() {
        int w = Mirror.width(), h = Mirror.height();
        int sw = stage.getWidth(), sh = stage.getHeight();
        if (w <= 0 || h <= 0 || sw <= 0 || sh <= 0) return;
        float scale = Math.min((float) sw / w, (float) sh / h);
        int pw = Math.round(w * scale), ph = Math.round(h * scale);
        ViewGroup.LayoutParams lp = picture.getLayoutParams();
        if (lp.width != pw || lp.height != ph) {
            picture.setLayoutParams(new FrameLayout.LayoutParams(pw, ph, Gravity.CENTER));
        }
    }

    private boolean onPictureTouch(View v, MotionEvent e) {
        TouchService t = touch();
        if (t == null || v.getWidth() == 0 || v.getHeight() == 0) return true;
        float x = e.getX() / v.getWidth() * Mirror.phoneWidth();
        float y = e.getY() / v.getHeight() * Mirror.phoneHeight();
        switch (e.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                t.touchDown(x, y);
                break;
            case MotionEvent.ACTION_MOVE:
                t.touchMove(x, y);
                break;
            case MotionEvent.ACTION_UP:
            case MotionEvent.ACTION_CANCEL:
                t.touchUp(x, y);
                break;
            default:
                break;
        }
        return true;
    }

    /** The touch service, or null with a hint on screen when it is switched off. */
    private TouchService touch() {
        TouchService t = TouchService.get();
        if (t == null && Mirror.isCapturing()) {
            message.setText("Touch is off.\nOn your phone, open Truck Mirror and turn on touch control.");
            message.setVisibility(View.VISIBLE);
            message.postDelayed(() -> {
                if (Mirror.isCapturing()) message.setVisibility(View.GONE);
            }, 4000);
        }
        return t;
    }

    @Override
    public void onSurfaceTextureAvailable(SurfaceTexture st, int width, int height) {
        onMirrorChanged();
    }

    @Override
    public void onSurfaceTextureSizeChanged(SurfaceTexture st, int width, int height) {}

    @Override
    public boolean onSurfaceTextureDestroyed(SurfaceTexture st) {
        Mirror.setSurface(null);
        if (surface != null) {
            surface.release();
            surface = null;
        }
        return true;
    }

    @Override
    public void onSurfaceTextureUpdated(SurfaceTexture st) {}

    private TextView button(String label, Runnable action) {
        TextView b = new TextView(this);
        b.setText(label);
        b.setTextColor(Color.WHITE);
        b.setTextSize(TypedValue.COMPLEX_UNIT_SP, 28);
        b.setGravity(Gravity.CENTER);
        b.setOnClickListener(v -> action.run());
        return b;
    }

    private static LinearLayout.LayoutParams weighted() {
        return new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1);
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }
}
