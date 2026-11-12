package com.truckmirror;

import android.accessibilityservice.AccessibilityService;
import android.accessibilityservice.GestureDescription;
import android.accessibilityservice.GestureDescription.StrokeDescription;
import android.content.Intent;
import android.graphics.Path;
import android.view.accessibility.AccessibilityEvent;

/**
 * Replays touches from the truck screen on the phone. A finger on the truck is
 * sent as one stroke in short pieces, so drags follow the finger instead of
 * arriving all at once when it lifts.
 */
public class TouchService extends AccessibilityService {
    /** How long each piece of a stroke takes to play, in milliseconds. */
    private static final long PIECE_MS = 16;

    private static TouchService instance;

    private StrokeDescription stroke;
    private boolean down;      // a finger is down and more pieces may follow
    private boolean playing;   // a piece is being played by the system
    private boolean moved, lifted;
    private float lastX, lastY, nextX, nextY;
    private int generation;    // tells a stale callback from the current stroke's

    public static TouchService get() {
        return instance;
    }

    @Override
    protected void onServiceConnected() {
        instance = this;
    }

    @Override
    public boolean onUnbind(Intent intent) {
        if (instance == this) instance = null;
        return super.onUnbind(intent);
    }

    @Override
    public void onDestroy() {
        if (instance == this) instance = null;
        super.onDestroy();
    }

    @Override
    public void onAccessibilityEvent(AccessibilityEvent event) {}

    @Override
    public void onInterrupt() {}

    public void back() {
        performGlobalAction(GLOBAL_ACTION_BACK);
    }

    public void home() {
        performGlobalAction(GLOBAL_ACTION_HOME);
    }

    public void recents() {
        performGlobalAction(GLOBAL_ACTION_RECENTS);
    }

    public void touchDown(float x, float y) {
        reset();
        x = clampX(x);
        y = clampY(y);
        down = true;
        lastX = nextX = x;
        lastY = nextY = y;
        Path p = new Path();
        p.moveTo(x, y);
        stroke = new StrokeDescription(p, 0, PIECE_MS, true);
        play();
    }

    public void touchMove(float x, float y) {
        if (!down) return;
        nextX = clampX(x);
        nextY = clampY(y);
        moved = true;
        if (!playing) playNext();
    }

    public void touchUp(float x, float y) {
        if (!down) return;
        nextX = clampX(x);
        nextY = clampY(y);
        lifted = true;
        if (!playing) playNext();
    }

    /** Sends the newest finger position, skipping any it passed through meanwhile. */
    private void playNext() {
        if (!down || (!moved && !lifted)) return;
        Path p = new Path();
        p.moveTo(lastX, lastY);
        p.lineTo(nextX, nextY);
        boolean last = lifted;
        try {
            stroke = stroke.continueStroke(p, 0, PIECE_MS, !last);
        } catch (RuntimeException e) {
            reset();
            return;
        }
        lastX = nextX;
        lastY = nextY;
        moved = false;
        if (last) {
            lifted = false;
            down = false;
        }
        play();
    }

    private void play() {
        final int mine = generation;
        playing = true;
        boolean accepted;
        try {
            accepted = dispatchGesture(
                    new GestureDescription.Builder().addStroke(stroke).build(),
                    new GestureResultCallback() {
                        @Override
                        public void onCompleted(GestureDescription g) {
                            if (mine != generation) return;
                            playing = false;
                            playNext();
                        }

                        @Override
                        public void onCancelled(GestureDescription g) {
                            if (mine == generation) reset();
                        }
                    }, null);
        } catch (RuntimeException e) {
            accepted = false;
        }
        if (!accepted) reset();
    }

    private void reset() {
        generation++;
        stroke = null;
        down = playing = moved = lifted = false;
    }

    private static float clampX(float x) {
        return Math.max(0, Math.min(x, Mirror.phoneWidth() - 1));
    }

    private static float clampY(float y) {
        return Math.max(0, Math.min(y, Mirror.phoneHeight() - 1));
    }
}
