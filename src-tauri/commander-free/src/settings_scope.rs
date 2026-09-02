use super::SettingsScope;

pub(super) struct SystemStateScopeRule {
    pub(super) path: &'static str,
    pub(super) scope: SettingsScope,
}

/// Exhaustive SystemState leaf ownership. Each serialized leaf is classified
/// exactly once; no fallback scope is allowed.
pub(super) const SYSTEM_STATE_SCOPE_TABLE: &[SystemStateScopeRule] = &[
    SystemStateScopeRule {
        path: "device.cpu",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.ramGb",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.gpu",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.disks",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.macAddresses",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.serialNumber",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.biosVersion",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.osBuild",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.domain",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.timeZone",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.systemLocale",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.users",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.runtimes",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.windowsActivated",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.bitlockerStatus",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.lastUpdateAt",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.pendingUpdatesCount",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.hostname",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.osName",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.osVersion",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.deviceType",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.ram",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.isAdmin",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.gpuDetection.gpuFound",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.gpuDetection.gpuName",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.gpuDetection.driverVersion",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.gpuDetection.vramMb",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.gpuDetection.computeCapability",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.gpuDetection.cudaFound",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.gpuDetection.cudaPath",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "device.gpuDetection.lastCheckedAt",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.telemetry.windowsDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.telemetry.officeDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.telemetry.powershell7Disabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.telemetry.copilotDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.telemetry.activityHistoryDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.telemetry.locationTrackingDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.telemetry.windowsSuggestionsDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.clipboard.historyDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.clipboard.cloudSyncDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.clipboard.pasteMonitorEnabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.clipboard.pasteMonitorCategories.cloudApi",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.clipboard.pasteMonitorCategories.aiApi",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.clipboard.pasteMonitorCategories.devTools",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.clipboard.pasteMonitorCategories.paymentComms",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.clipboard.pasteMonitorCategories.keysAndCrypto",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.clipboard.pasteMonitorCategories.personalData",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.clipboard.pasteMonitorCategories.maliciousCommand",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.clipboard.pasteMonitorCategories.unicode",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.clipboard.pasteMonitorCryptoSwapEnabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.clipboard.pasteMonitorAutoClearEnabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.clipboard.pasteMonitorAutoClearSeconds",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.clipboard.pasteMonitorReportToFleet",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.recentFilesDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.jumpListsDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.thumbnailCacheDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.pagefileDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.recallSnapshotsDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.typingInsightsDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.advertisingIdDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.tailoredExperiencesDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.officeLoggingDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.diagnosticEventTracingDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.rdpIdleDisconnectEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.rdpIdleDisconnectTimeout",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.rdpIdleWarningSeconds",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.rdpClearCacheOnDisconnect",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.rdpRemoveCredsOnDisconnect",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.rdpSaveLog",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.rdpDismountVaultsOnDisconnect",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.quickAccessRecentDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.quickAccessFrequentDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.runMruDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.searchHistoryDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.tracking.terminalHistoryDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.lockscreen.privacyDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.appCapabilities.webcam",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.appCapabilities.microphone",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.appCapabilities.location",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.appCapabilities.contacts",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.appCapabilities.calendar",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.appCapabilities.callHistory",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.appCapabilities.phoneCall",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.appCapabilities.email",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.appCapabilities.messaging",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.appCapabilities.radios",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.appCapabilities.bluetoothSync",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.appCapabilities.appDiagnostics",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.appCapabilities.documents",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.appCapabilities.pictures",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.appCapabilities.videos",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.appCapabilities.fileSystem",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.appCapabilities.notifications",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.appCapabilities.gazeInput",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.appCapabilities.userAccountInformation",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.internetCommunication.restrictedEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.privacyProtectionEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.setupCompletionNagsDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.acquisitionWatchEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.privacyShield.notifyMode",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.privacyShield.fleetMonitoringEnabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.privacyShield.fleetManaged",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.privacyShield.fleetNotificationLimit",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.privacyShield.fleetNotificationWindowSeconds",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.privacyShield.gazeDetectionEnabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.privacyShield.antiPeepingEnabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.privacyShield.cameraHunterEnabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.privacyShield.confidenceThreshold",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.privacyShield.wakeDelaySeconds",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.privacyShield.blurOpacity",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.privacyShield.modelSize",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.privacyShield.detectionBufferFrames",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.privacyShield.captureOnDevice",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.privacyShield.captureOnMultiFace",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.privacyShield.captureSpeed",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.privacyShield.deviceWakeMultiplier",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.privacyShield.multiFaceWakeMultiplier",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.privacyShield.autostart",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.decoyMonitor.enabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.decoyMonitor.enrolledPaths",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.decoyMonitor.readAuditEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.decoyMonitor.fleetAlertEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.ransomwareMonitor.enabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.ransomwareMonitor.threshold",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.ransomwareMonitor.windowSeconds",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.ransomwareMonitor.alertCooldownSeconds",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.ransomwareMonitor.attributionMinFiles",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.ransomwareMonitor.customWatchDirs",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.ransomwareMonitor.action",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.ransomwareMonitor.reportToFleet",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.remoteAccessMonitor.enabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.remoteAccessMonitor.tools",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.authAnomalyMonitor.enabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.authAnomalyMonitor.failedBurstThreshold",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.authAnomalyMonitor.failedBurstWindowSecs",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.authAnomalyMonitor.workStartHour",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.authAnomalyMonitor.workEndHour",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.authAnomalyMonitor.workDays",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.authAnomalyMonitor.timeBasis",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.authAnomalyMonitor.detectRdp",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.authAnomalyMonitor.detectNewAccounts",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.authAnomalyMonitor.detectOffHours",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.authAnomalyMonitor.alertDebounceSecs",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.authAnomalyMonitor.reportToFleet",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.usbSecurity.monitorEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.usbSecurity.hidGuardEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.usbSecurity.meteringEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.usbSecurity.autoSandboxEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.screenCapture.detectionEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.screenCapture.protectWindow",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.screenCapture.reportToFleet",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.coercionPhrase.enabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.coercionPhrase.phrases",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.selfDestruct.enabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.selfDestruct.steps",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.selfDestruct.excludeBrowsers",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.selfDestruct.deactivateLicenseFirst",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.selfDestruct.shutdownSystem",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.selfDestruct.rebootToUsbEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.selfDestruct.shredFolders",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.selfDestruct.usersToRemove",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.selfDestruct.cryptoEraseVeracryptPaths",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.selfDestruct.cryptoEraseVeracryptDevices",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.selfDestruct.cryptoEraseBitlockerDrives",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.startupPin.enabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.startupPin.realHash",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.startupPin.decoyHash",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.startupPin.destroyHash",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "privacy.distressPhrases",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "privacy.browserExtensions",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.security.defenderDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.windowsUpdateDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.uacDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.usbWriteProtect",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.usbStorageLockdown",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.consumerFeaturesDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.remoteAssistanceDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.anonymousSamEnumerationBlocked",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.vbsDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.bitlockerAutoEncryptDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.wpbtDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.smartScreenDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.oobeBypassEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.gameDvrDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.firefoxHardeningEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.braveHardeningEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.chromeHardeningEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.edgeHardeningEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.universalExtensionsDeployed",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.browserAutoUpdateForced",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.copilotAiRemoved",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.systemRestoreOff",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.recallOff",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.crashDumpsOff",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.clipboardHistoryOff",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.security.requirePwOnResume",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.security.kernelDmaProtect",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.shredPasses",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.shredMediaAwareEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.shredMftSlackEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.ramSpillControlEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.bitlockerTpmPinEnforce",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.acquisitionDriverBlocklist",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.forensicToolBlock",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.lidClosePowerOff",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.depEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.aslrMandatory",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.aslrBottomUp",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.cfgEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.heapIntegrity",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.sehopEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.asrRulesEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.controlledFolderAccessEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.security.networkProtectionEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.os.superfetchDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.os.prefetchDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.os.hibernationDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.os.fastStartupDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.os.ntfsOptimizationsEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.os.detailedBsodEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.os.memoryCompressionDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.os.win32PrioritySeparation",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.os.desktopShellPriorityEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.os.serviceTimeoutsOptimized",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.os.reservedStorageDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.os.automaticMaintenanceDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.os.win32LongPathsEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.os.smbBandwidthThrottlingDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.classicContextMenu",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.fileExtensionsVisible",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.hiddenFilesVisible",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.galleryHomeRemoved",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.bingSearchDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.backgroundAppsDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.notificationsDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.endTaskOnTaskbar",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.folderTypeDiscoveryDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.shortcutSuffixRemoved",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.autoPlayDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.taskbarDebloated",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.startRecommendationsDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.lowDiskCheckDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.explorerOpensThisPc",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.syncProviderNotificationsHidden",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.transparencyDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.fullPathInTitleBar",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.desktopIconThisPc",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.desktopIconRecycleBin",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.desktopIconUserFiles",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.desktopIconNetwork",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.desktopIconControlPanel",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.shortcutArrowRemoved",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.snapAssistFlyoutDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.explorerCompactMode",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.explorerCheckboxesEnabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.windowShakeDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.clockSecondsVisible",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.ui.powerShell7Default",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.bootKernel.tsxEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.bootKernel.firstLogonAnimationDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.bootKernel.startupSoundDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.bootKernel.autoRestartSignonDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.bootKernel.autoRebootOnBsodDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.bootKernel.smallMemoryDumpEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.rdp.keepAlive",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.rdp.noTimeouts",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.rdp.qosPriority",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.rdp.incomingIdleTimeoutEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.rdp.incomingIdleTimeoutSeconds",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.rdp.incomingIdleTimeoutMinutes",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.rdp.incomingDismountOnEmpty",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.rdp.incomingSignOffOnDisconnect",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.server.persistentRdpAnimations",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.server.ctrlAltDelDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.server.lastSignedInUserHidden",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.server.consoleInactivityLock",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.server.shutdownTrackerDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.server.serverManagerAtLogonDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.server.ieEnhancedSecurityDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.server.wdigestBlocked",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.server.lsaProtectionEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.server.legacyNtlmBlocked",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.server.smbSigningRequired",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.server.smb1Disabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.server.remoteRegistryDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.performance.mmcssGamingProfile",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.performance.keyboardLatencyOptimised",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.performance.numLockOnBoot",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.performance.gpuSchedulingEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.performance.svcHostSplitOptimised",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.performance.accessibilityShortcutsDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.performance.instantMenuDelay",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.performance.mouseAccelerationDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.performance.autocorrectDisabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.performance.enthusiastModeEnabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.performance.wallpaperFullQuality",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "tweaks.gpu.amdUlpsDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.gpu.amdPowerGatingDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.gpu.amdVideoClockGatingDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.gpu.amdAspmDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.gpu.nvidiaDynamicPstateDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.gpu.nvidiaAsyncPstatesDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.gpu.intelAsyncFlipsDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.gpu.intelAdaptiveVsyncDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.power.usbSelectiveSuspendDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.power.cpuThrottlingDisabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.powerPlan",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.aiComponentCleanup",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "tweaks.maintenanceRuns",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "network.dns.provider",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "network.dns.ipv4Preference",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "network.dns.swissFirewallConfig.dohId",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "network.dns.swissFirewallConfig.deviceName",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "network.dns.controlDFilterSlug",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "network.dns.censorshipProtection",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "network.hosts.enabledBlocklists",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "network.firewall.lockdownMode",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "network.firewall.managedRules",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "network.vpnKillSwitch.armed",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "network.vpnKillSwitch.provider",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "network.vpnKillSwitch.pollIntervalSecs",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "network.wifiGuard.enabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "network.wifiGuard.learningWindowSecs",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "network.wifiGuard.learningUntil",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "network.wifiGuard.pollIntervalSecs",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "network.wifiGuard.alertDebounceSecs",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "network.wifiGuard.baseline",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "network.wifiGuard.reportToFleet",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "identity.branding.companyName",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "identity.branding.productName",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "identity.branding.pcName",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "identity.branding.manufacturer",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "identity.branding.supportUrl",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "identity.stealthModeEnabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "identity.hideServerApps",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "identity.hideWinCommander",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "identity.hideWinCommanderHotkey",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "identity.flowsEnabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "identity.advancedToolsEnabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "identity.riskMatrixEnabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "identity.moreProductsEnabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "identity.hideBackendAppsList",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "apps.requiredApps",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.blockedApps",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.edgeRemoved",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.onedriveRemoved",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.teamsRemoved",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.removedAppx",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.deprovisionedAppx",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.autoUpdate",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.autoUpdateManifestOnly",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.pinnedVersions",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.scanIntervalMinutes",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.inventory.lastScanAt",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.inventory.scanDurationMs",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.inventory.manifestApps",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.inventory.otherApps",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.inventory.pendingUpdates",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.inventory.essentials.meshVpn.installed",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.inventory.essentials.meshVpn.version",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.inventory.essentials.meshVpn.connected",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.inventory.essentials.productivityEngine.installed",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.inventory.essentials.productivityEngine.running",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.inventory.essentials.winget.installed",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.inventory.essentials.winget.version",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.inventory.summary.totalInstalled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.inventory.summary.manifestInstalled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.inventory.summary.manifestTotal",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.inventory.summary.manifestMissing",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.inventory.summary.otherInstalled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.inventory.summary.updatesAvailable",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "apps.inventory.summary.essentialsOk",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "productivity.trackerEnabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "productivity.productivityEngineStealthEnabled",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "productivity.excludeAfk",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "productivity.defaultRange",
        scope: SettingsScope::User,
    },
    SystemStateScopeRule {
        path: "serverApps.apps",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "security.drivers.watchEnabled",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "security.drivers.watchIntervalSecs",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "security.drivers.scanOnStartup",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "security.requireAllDeviceAlertsInFleet",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "security.metricAlertReporting.systemReportToFleet",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "security.metricAlertReporting.networkReportToFleet",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "security.metricAlertReporting.policy.evaluationIntervalSecs",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "security.metricAlertReporting.policy.sustainedSecs",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "security.metricAlertReporting.policy.cooldownSecs",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "security.metricAlertReporting.policy.resetPct",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "security.metricAlertReporting.policy.cpuThresholdPct",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "security.metricAlertReporting.policy.ramThresholdPct",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "security.metricAlertReporting.policy.uploadThresholdMbS",
        scope: SettingsScope::Machine,
    },
    SystemStateScopeRule {
        path: "security.metricAlertReporting.policy.downloadThresholdMbS",
        scope: SettingsScope::Machine,
    },
];
