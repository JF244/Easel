# Truck Mirror

Shows an Android phone's screen on a car display through Android Auto, and lets
the car's touchscreen control the phone. No root. Unrelated to Easel itself; it
lives here because this is the repo it was built in.

Status: it builds. It has **not** been run against a car yet.

## How it works

- **Phone:** `MainActivity` asks for screen-capture permission and starts
  `CaptureService`, which keeps one `MediaProjection` virtual display alive.
- **Truck:** `car/MirrorCarService` is what Android Auto lists. It hosts
  `car/MirrorCarActivity`, which draws the captured screen and has Back, Home
  and Recents buttons down the left edge.
- **Touch:** `TouchService` is an accessibility service that replays touches
  from the truck screen on the phone.

It uses the unofficial Android Auto SDK (`com.github.martoreto:aauto-sdk`),
which Google can break with any Android Auto update.

## Limits

- Netflix, Disney+ and other protected apps show black: Android blocks screen
  capture of them.
- One finger only, so no pinch zoom.
- The phone has to stay unlocked; locking it stops the screen share.

## Installing

Android Auto only lists apps it believes came from the Play Store, so a plain
install will not show up in the truck. Reported to work without root:

- Install the APK with **KingInstaller** (alone, or with its Shizuku option).
- Or turn on **developer mode** in the AAWireless companion app.

`adb install -i com.android.vending` is reported not to work on Android Auto 17.7.

After installing, open Android Auto settings on the phone, go to **Customize
launcher** and tick Truck Mirror.

## Building

Needs the Android SDK (platform 35, build-tools 35) and network access to
`dl.google.com` and `jitpack.io`.

```
gradle assembleDebug
```

The APK lands in `app/build/outputs/apk/debug/`. `app/sideload.keystore` is a
fixed, non-secret signing key so each build can update the last.
