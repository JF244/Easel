package com.truckmirror.car;

import com.google.android.apps.auto.sdk.CarActivity;
import com.google.android.apps.auto.sdk.CarActivityService;

/** Entry point Android Auto binds to when Truck Mirror is opened on the truck. */
public class MirrorCarService extends CarActivityService {
    @Override
    public Class<? extends CarActivity> getCarActivity() {
        return MirrorCarActivity.class;
    }
}
