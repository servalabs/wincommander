import type { PrivacyShieldSettings } from "../types/settings";

export const defaultPrivacyShieldConfig = () => ({
  blurOnLookAway: true, blurOnMultipleFaces: true, blurOnCamera: true,
  captureOnDevice: false, captureOnMultiFace: false, sendIncidentCapturesToFleet: false,
  modelLevel: "medium", confidence: 0.5, overlayOpacity: 200,
  wakeDelayMs: 150, deviceWakeMultiplier: 5, multiFaceWakeMultiplier: 5,
  bufferFrames: 2, captureSpeed: 1,
});

export type PrivacyShieldConfig = ReturnType<typeof defaultPrivacyShieldConfig>;

export function hydratePrivacyShieldConfig(previous: PrivacyShieldConfig, settings: PrivacyShieldSettings): PrivacyShieldConfig {
  return {
    blurOnLookAway: settings.gazeDetectionEnabled ?? previous.blurOnLookAway,
    blurOnMultipleFaces: settings.antiPeepingEnabled ?? previous.blurOnMultipleFaces,
    blurOnCamera: settings.cameraHunterEnabled ?? previous.blurOnCamera,
    captureOnDevice: settings.captureOnDevice ?? previous.captureOnDevice,
    captureOnMultiFace: settings.captureOnMultiFace ?? previous.captureOnMultiFace,
    sendIncidentCapturesToFleet: settings.sendIncidentCapturesToFleet ?? previous.sendIncidentCapturesToFleet,
    modelLevel: settings.modelSize ?? previous.modelLevel,
    confidence: settings.confidenceThreshold ?? previous.confidence,
    overlayOpacity: settings.blurOpacity ?? previous.overlayOpacity,
    wakeDelayMs: settings.wakeDelaySeconds ?? previous.wakeDelayMs,
    deviceWakeMultiplier: settings.deviceWakeMultiplier ?? previous.deviceWakeMultiplier,
    multiFaceWakeMultiplier: settings.multiFaceWakeMultiplier ?? previous.multiFaceWakeMultiplier,
    bufferFrames: settings.detectionBufferFrames ?? previous.bufferFrames,
    captureSpeed: settings.captureSpeed ?? previous.captureSpeed,
  };
}

export function privacyShieldConfigPatch(config: PrivacyShieldConfig) {
  return {
    gazeDetectionEnabled: config.blurOnLookAway,
    antiPeepingEnabled: config.blurOnMultipleFaces,
    cameraHunterEnabled: config.blurOnCamera,
    captureOnDevice: config.captureOnDevice,
    captureOnMultiFace: config.captureOnMultiFace,
    sendIncidentCapturesToFleet: config.sendIncidentCapturesToFleet,
    confidenceThreshold: config.confidence,
    blurOpacity: config.overlayOpacity,
    wakeDelaySeconds: Math.round(config.wakeDelayMs),
    modelSize: config.modelLevel as NonNullable<PrivacyShieldSettings["modelSize"]>,
    detectionBufferFrames: config.bufferFrames,
    captureSpeed: config.captureSpeed,
    deviceWakeMultiplier: config.deviceWakeMultiplier,
    multiFaceWakeMultiplier: config.multiFaceWakeMultiplier,
  };
}
