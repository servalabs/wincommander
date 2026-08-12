import { Dialog, Button, FormGroup, InputGroup, HTMLSelect, Icon, CheckboxControl } from "@/components/ui/bp";
import { useState, useMemo, useCallback, useEffect } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import useBackend from "../../hooks/useBackend";
import type { EncryptionPartition } from "../../hooks/useBackend";
import useVisibility from "../../hooks/useVisibility";
import { useTheme } from "../../context/ThemeContext";
import './CreateVolumeWizard.css';

type FSType = "None" | "NTFS" | "FAT" | "ExFAT";
type SizeUnit = "M" | "G" | "T";
type EncAlgo = "AES" | "Serpent" | "Twofish" | "Camellia" | "Kuznyechik" | "AES(Twofish)" | "AES(Twofish(Serpent))" | "Serpent(AES)" | "Serpent(Twofish(AES))" | "Twofish(Serpent)" | "Camellia(Kuznyechik)" | "Kuznyechik(Twofish)" | "Camellia(Serpent)" | "Kuznyechik(AES)" | "Kuznyechik(Serpent(Camellia))";
type HashAlgo = "sha-512" | "sha-256" | "whirlpool" | "blake2s-256" | "argon2id" | "streebog";

type ApplyStatus = "ok" | "error" | "running" | "pending";

interface ApplyLogItem {
    label: string;
    status: ApplyStatus;
    detail?: string;
}

interface CreateVolumeWizardProps {
    isOpen: boolean;
    onClose: () => void;
    onCreated: () => void;
}

const ENC_ALGOS: { value: EncAlgo; label: string }[] = [
    { value: "AES", label: "AES (Recommended)" },
    { value: "Serpent", label: "Serpent" },
    { value: "Twofish", label: "Twofish" },
    { value: "Camellia", label: "Camellia" },
    { value: "Kuznyechik", label: "Kuznyechik" },
    { value: "AES(Twofish)", label: "AES-Twofish (Cascade)" },
    { value: "AES(Twofish(Serpent))", label: "AES-Twofish-Serpent (Triple Cascade)" },
    { value: "Serpent(AES)", label: "Serpent-AES" },
    { value: "Serpent(Twofish(AES))", label: "Serpent-Twofish-AES" },
    { value: "Twofish(Serpent)", label: "Twofish-Serpent" },
    { value: "Camellia(Kuznyechik)", label: "Camellia-Kuznyechik" },
    { value: "Kuznyechik(Twofish)", label: "Kuznyechik-Twofish" },
    { value: "Camellia(Serpent)", label: "Camellia-Serpent" },
    { value: "Kuznyechik(AES)", label: "Kuznyechik-AES" },
    { value: "Kuznyechik(Serpent(Camellia))", label: "Kuznyechik-Serpent-Camellia" },
];

const HASH_ALGOS: { value: HashAlgo; label: string }[] = [
    { value: "sha-512", label: "SHA-512 (Recommended)" },
    { value: "sha-256", label: "SHA-256" },
    { value: "whirlpool", label: "Whirlpool" },
    { value: "blake2s-256", label: "BLAKE2s-256" },
    { value: "argon2id", label: "Argon2id" },
    { value: "streebog", label: "Streebog" },
];

const FS_TYPES: { value: FSType; label: string; desc: string }[] = [
    { value: "None", label: "None", desc: "Leave the decrypted volume unformatted" },
    { value: "NTFS", label: "NTFS", desc: "Windows native, supports large files" },
    { value: "FAT", label: "FAT32", desc: "Cross-platform compatible" },
    { value: "ExFAT", label: "exFAT", desc: "Large file support, cross-platform" },
];

const MAX_PIM = 2_147_468;

function creationPimPasswordValid(password: string, pim: string, hash: HashAlgo): boolean {
    if (!pim) return true;
    const value = Number(pim);
    if (!Number.isInteger(value) || value < 1 || value > MAX_PIM) return false;
    if (!password) return true;
    const defaultPim = hash === "argon2id" ? 12 : 485;
    return value >= defaultPim || new TextEncoder().encode(password).length >= 20;
}

function getPasswordStrength(pw: string): { score: number; label: string; color: string } {
    if (!pw) return { score: 0, label: "", color: "var(--color-border)" };

    // Automatic Likely Unbreakable
    if (pw.length >= 24) return { score: 6, label: "Likely Unbreakable", color: "var(--color-success)" };

    // Weak until 8 chars
    if (pw.length < 8) return { score: 1, label: "Weak", color: "var(--color-danger)" };

    let score = 0;
    if (pw.length >= 8) score++;
    if (pw.length >= 16) score++;
    if (/[A-Z]/.test(pw)) score++;
    if (/[0-9]/.test(pw)) score++;
    if (/[^A-Za-z0-9]/.test(pw)) score++;

    // We already handled < 8 as Weak, so map the score for >= 8
    // Max score here is 5
    if (score <= 2) return { score: 2, label: "Fair", color: "var(--color-warning)" };
    if (score <= 3) return { score: 3, label: "Good", color: "var(--color-warning)" };
    if (score <= 4) return { score: 4, label: "Strong", color: "var(--color-success)" };
    return { score: 5, label: "Very Strong", color: "var(--color-accent)" };
}

function CreateVolumeWizard({ isOpen, onClose, onCreated }: CreateVolumeWizardProps) {
    const { theme } = useTheme();
    const visibility = useVisibility();
    const isAdvanced = visibility.isVisible({ minDensity: "expert", capability: ["safeguards"] });

    const { createVolume, createDualVolume, getEncryptionPartitions } = useBackend();

    // Step tracking
    const [stepIndex, setStepIndex] = useState(0);

    // Volume kind: a standard volume, or a two-password pair — one outer
    // volume plus an inner volume in its free space.
    const [volumeType, setVolumeType] = useState<"standard" | "dual">("standard");
    const [targetKind, setTargetKind] = useState<"file" | "device">("file");
    const [partitions, setPartitions] = useState<EncryptionPartition[]>([]);
    const [selectedDevicePath, setSelectedDevicePath] = useState("");
    const [deviceConfirmation, setDeviceConfirmation] = useState("");

    // Form state
    const [volumeFolder, setVolumeFolder] = useState("");
    const [volumeName, setVolumeName] = useState("");

    const sep = volumeFolder.includes("/") ? "/" : "\\";
    const volumePath = volumeFolder ? `${volumeFolder.replace(/[\\/]+$/, "")}${sep}${volumeName}` : "";

    const [sizeValue, setSizeValue] = useState(500);
    const [sizeUnit, setSizeUnit] = useState<SizeUnit>("M");
    const [encAlgo, setEncAlgo] = useState<EncAlgo>("AES");
    const [hashAlgo, setHashAlgo] = useState<HashAlgo>("sha-512");
    const [filesystem, setFilesystem] = useState<FSType>("NTFS");
    const [password, setPassword] = useState("");
    const [passwordConfirm, setPasswordConfirm] = useState("");
    const [showPassword, setShowPassword] = useState(false);
    const [keyfile, setKeyfile] = useState("");
    const [pim, setPim] = useState("");
    const [secondKeyfile, setSecondKeyfile] = useState("");
    const [secondPim, setSecondPim] = useState("");
    const [quickFormat, setQuickFormat] = useState(true);
    const [dynamicFormat, setDynamicFormat] = useState(false);

    // Two-password volume: a second password + an inner size carved from the host's free space.
    const isDual = volumeType === "dual";
    const [secondPassword, setSecondPassword] = useState("");
    const [secondPasswordConfirm, setSecondPasswordConfirm] = useState("");
    const [secondSizeValue, setSecondSizeValue] = useState(100);

    // Apply state
    const [applyLog, setApplyLog] = useState<ApplyLogItem[]>([]);
    const [applying, setApplying] = useState(false);
    const [done, setDone] = useState(false);
    const selectedPartition = partitions.find(partition => partition.devicePath === selectedDevicePath);

    useEffect(() => {
        if (!isOpen) return;
        let cancelled = false;
        void getEncryptionPartitions().then(result => {
            if (!cancelled && result?.success && result.data?.partitions) {
                setPartitions(result.data.partitions);
            }
        });
        return () => { cancelled = true; };
    }, [getEncryptionPartitions, isOpen]);

    const steps = useMemo(() => {
        const base = [
            { id: "location", title: "Location" },
            { id: "size", title: "Size" },
            ...(isAdvanced ? [{ id: "encryption", title: "Encryption" }] : []),
            { id: "filesystem", title: "Filesystem" },
            { id: "password", title: "Password" },
            { id: "summary", title: "Create" },
        ];
        return base;
    }, [isAdvanced]);

    const currentStep = steps[stepIndex];

    const canNext = useMemo(() => {
        switch (currentStep?.id) {
            case "location": return targetKind === "file"
                ? volumeFolder.trim().length > 0 && volumeName.trim().length > 0
                : Boolean(selectedPartition?.safeForCreation && deviceConfirmation === selectedPartition.confirmationToken);
            case "size": {
                if (targetKind === "file") return sizeValue > 0 && (!isDual || (secondSizeValue > 0 && secondSizeValue < sizeValue));
                if (!isDual) return true;
                const multiplier = sizeUnit === "M" ? 1024 ** 2 : sizeUnit === "G" ? 1024 ** 3 : 1024 ** 4;
                return Boolean(selectedPartition && secondSizeValue > 0 && secondSizeValue * multiplier < selectedPartition.sizeBytes);
            }
            case "encryption": return true;
            case "filesystem": return true;
            case "password": {
                const primaryCredentialValid = (password.length >= 8 && password === passwordConfirm) || (!password && Boolean(keyfile));
                const primaryPimValid = creationPimPasswordValid(password, pim, hashAlgo);
                if (isDual) {
                    const hiddenCredentialValid = (secondPassword.length >= 8 && secondPassword === secondPasswordConfirm) || (!secondPassword && Boolean(secondKeyfile));
                    const hiddenPimValid = creationPimPasswordValid(secondPassword, secondPim, hashAlgo);
                    return primaryCredentialValid && primaryPimValid && hiddenCredentialValid && hiddenPimValid
                        && (password !== secondPassword || keyfile !== secondKeyfile);
                }
                return primaryCredentialValid && primaryPimValid;
            }
            case "summary": return false;
            default: return false;
        }
    }, [currentStep, deviceConfirmation, hashAlgo, keyfile, password, passwordConfirm, pim, secondKeyfile, secondPim, selectedPartition, sizeUnit, sizeValue, targetKind, volumeFolder, volumeName, isDual, secondSizeValue, secondPassword, secondPasswordConfirm]);

    const handleBrowse = async () => {
        try {
            const selected = await open({
                directory: true,
                multiple: false,
                defaultPath: volumeFolder || undefined,
            });
            if (selected && typeof selected === "string") {
                setVolumeFolder(selected);
            }
        } catch { }
    };

    const handleBrowseKeyfile = async () => {
        try {
            const selected = await open({ multiple: false });
            if (selected && typeof selected === "string") {
                setKeyfile(selected);
            }
        } catch { }
    };

    const handleBrowseSecondKeyfile = async () => {
        try {
            const selected = await open({ multiple: false });
            if (selected && typeof selected === "string") setSecondKeyfile(selected);
        } catch { }
    };

    const logStep = useCallback((label: string, status: ApplyStatus, detail?: string) => {
        setApplyLog(prev => {
            const idx = prev.findIndex(i => i.label === label);
            if (idx >= 0) {
                const next = [...prev];
                next[idx] = { label, status, detail };
                return next;
            }
            return [...prev, { label, status, detail }];
        });
    }, []);

    const handleCreate = useCallback(async () => {
        setApplying(true);
        const sizeStr = `${sizeValue}${sizeUnit}`;
        const label = isDual ? "Creating outer and hidden volumes" : `Creating encrypted ${targetKind === "device" ? "partition" : "container"}`;
        logStep(label, "running");
        try {
            const device = targetKind === "device" && selectedPartition ? {
                diskNumber: selectedPartition.diskNumber,
                partitionNumber: selectedPartition.partitionNumber,
                partitionGuid: selectedPartition.partitionGuid,
                offsetBytes: selectedPartition.offsetBytes,
                sizeBytes: selectedPartition.sizeBytes,
                diskUniqueId: selectedPartition.diskUniqueId,
                label: selectedPartition.label,
                confirmation: deviceConfirmation,
            } : undefined;
            const res = isDual
                ? await createDualVolume({
                    TargetKind: targetKind,
                    Path: targetKind === "file" ? volumePath : "",
                    HostSize: sizeStr,
                    SecondSize: `${secondSizeValue}${sizeUnit}`,
                    FirstPassword: password,
                    SecondPassword: secondPassword,
                    Encryption: encAlgo,
                    Hash: hashAlgo,
                    Filesystem: filesystem,
                    OuterKeyfiles: keyfile ? [keyfile] : [],
                    InnerKeyfiles: secondKeyfile ? [secondKeyfile] : [],
                    OuterPim: pim || undefined,
                    InnerPim: secondPim || undefined,
                    Device: device,
                })
                : await createVolume({
                    TargetKind: targetKind,
                    Path: targetKind === "file" ? volumePath : "",
                    Size: sizeStr,
                    Password: password,
                    Encryption: encAlgo,
                    Hash: hashAlgo,
                    Filesystem: filesystem,
                    Quick: quickFormat,
                    Dynamic: targetKind === "file" && dynamicFormat,
                    ...(keyfile ? { Keyfiles: [keyfile] } : {}),
                    ...(pim ? { Pim: pim } : {}),
                    Device: device,
                });
            if (res?.success === false) {
                logStep(label, "error", res.error || "Unknown error");
                setPassword("");
                setPasswordConfirm("");
                if (isDual) {
                    setSecondPassword("");
                    setSecondPasswordConfirm("");
                }
            } else {
                logStep(label, "ok");
                logStep("Done", "ok");
                setDone(true);
            }
        } catch (e: any) {
            logStep(label, "error", e?.message);
            setPassword("");
            setPasswordConfirm("");
            if (isDual) {
                setSecondPassword("");
                setSecondPasswordConfirm("");
            }
        }
        setApplying(false);
    }, [
        createVolume,
        createDualVolume,
        encAlgo,
        filesystem,
        hashAlgo,
        isDual,
        deviceConfirmation,
        dynamicFormat,
        secondPassword,
        secondKeyfile,
        secondPim,
        secondSizeValue,
        keyfile,
        logStep,
        password,
        pim,
        quickFormat,
        selectedPartition,
        sizeUnit,
        sizeValue,
        targetKind,
        volumePath,
    ]);

    // Whether any step has errored — used to unlock back/cancel during apply failure
    const hasError = applyLog.some(i => i.status === "error");

    const handleClose = useCallback(() => {
        if (applying && !hasError) return;
        setStepIndex(0);
        setVolumeFolder("");
        setVolumeName("");
        setTargetKind("file");
        setSelectedDevicePath("");
        setDeviceConfirmation("");
        setSizeValue(500);
        setSizeUnit("M");
        setEncAlgo("AES");
        setHashAlgo("sha-512");
        setFilesystem("NTFS");
        setPassword("");
        setPasswordConfirm("");
        setKeyfile("");
        setPim("");
        setSecondKeyfile("");
        setSecondPim("");
        setDynamicFormat(false);
        setVolumeType("standard");
        setSecondPassword("");
        setSecondPasswordConfirm("");
        setSecondSizeValue(100);
        setApplyLog([]);
        setApplying(false);
        setDone(false);
        if (done) onCreated();
        else onClose();
    }, [applying, done, hasError, onClose, onCreated]);

    const pwStrength = getPasswordStrength(password);

    const renderStepContent = () => {
        switch (currentStep?.id) {
            case "location":
                return (
                    <div className="wizard-step-content">
                        <div className="fs-grid" style={{ marginBottom: 16 }}>
                            <button type="button" className={`fs-card ${!isDual ? "selected" : ""}`} aria-pressed={!isDual} onClick={() => setVolumeType("standard")}>
                                <strong>Standard</strong>
                                <span>One encrypted volume</span>
                            </button>
                            <button type="button" className={`fs-card ${isDual ? "selected" : ""}`} aria-pressed={isDual} onClick={() => setVolumeType("dual")}>
                                <strong>Hidden + decoy</strong>
                                <span>Outer decoy and protected hidden volume</span>
                            </button>
                        </div>
                        <div className="fs-grid" style={{ marginBottom: 16 }}>
                            <button type="button" className={`fs-card ${targetKind === "file" ? "selected" : ""}`} aria-pressed={targetKind === "file"} onClick={() => setTargetKind("file")}>
                                <strong>File container</strong><span>Portable encrypted file</span>
                            </button>
                            <button type="button" className={`fs-card ${targetKind === "device" ? "selected" : ""}`} aria-pressed={targetKind === "device"} onClick={() => setTargetKind("device")}>
                                <strong>Partition / drive</strong><span>Erase and encrypt a whole partition</span>
                            </button>
                        </div>
                        {targetKind === "file" ? (<>
                            <p className="step-description">Choose where to save the container file. You can use any extension, or none at all.</p>
                            <FormGroup label="Container Folder" labelFor="vol-folder">
                                <InputGroup id="vol-folder" placeholder="C:\\Users\\You\\Documents" value={volumeFolder} autoComplete="off" onChange={e => setVolumeFolder(e.target.value)} onKeyDown={handleAdvanceFromInput} rightElement={<Button icon="folder-open" minimal aria-label="Browse for a container folder" onClick={handleBrowse} />} />
                            </FormGroup>
                            <FormGroup label="Container Name" labelFor="vol-name">
                                <InputGroup id="vol-name" placeholder="secure.hc" value={volumeName} autoComplete="off" onChange={e => setVolumeName(e.target.value)} onKeyDown={handleAdvanceFromInput} />
                            </FormGroup>
                        </>) : (<>
                            <div className="info-callout">
                                <Icon icon="warning-sign" intent="danger" />
                                <span><strong>Irreversible:</strong> the selected partition’s filesystem and every file on it will be replaced.</span>
                            </div>
                            <FormGroup label="Partition" labelFor="create-device">
                                <div className="partition-list" id="create-device" role="listbox" aria-label="Partition to erase and encrypt">
                                    {partitions.filter(partition => partition.safeForCreation).map(partition => (
                                        <button type="button" key={partition.devicePath} role="option" aria-selected={selectedDevicePath === partition.devicePath} className={`partition-row${selectedDevicePath === partition.devicePath ? " is-selected" : ""}`} onClick={() => { setSelectedDevicePath(partition.devicePath); setDeviceConfirmation(""); }}>
                                            <span className="partition-row-main">
                                                <span className="partition-row-title">{partition.label || "Unlabeled partition"}<span className="partition-row-size">{partition.size}</span></span>
                                                <span className="partition-row-sub">{partition.model} · Disk {partition.diskNumber} · Part {partition.partitionNumber} · {partition.filesystem || "Raw"}</span>
                                            </span>
                                        </button>
                                    ))}
                                </div>
                            </FormGroup>
                            {selectedPartition && (
                                <FormGroup label="Destructive confirmation" labelFor="device-confirmation" helperText={`Type exactly: ${selectedPartition.confirmationToken}`}>
                                    <InputGroup id="device-confirmation" value={deviceConfirmation} autoComplete="off" onChange={event => setDeviceConfirmation(event.target.value)} />
                                </FormGroup>
                            )}
                        </>)}
                    </div>
                );

            case "size":
                return (
                    <div className="wizard-step-content">
                        <p className="step-description">{targetKind === "device" ? "The outer volume uses the entire selected partition." : isDual ? "Set the total size and the hidden-volume size. Neither can be changed after creation." : "Set the total size of the container. This cannot be changed after creation."}</p>
                        {targetKind === "device" && selectedPartition && (
                            <div className="info-callout"><Icon icon="database" /><span>Entire partition: <strong>{selectedPartition.size}</strong> on Disk {selectedPartition.diskNumber}, Partition {selectedPartition.partitionNumber}</span></div>
                        )}
                        {targetKind === "file" && <div className="size-row">
                            <FormGroup label={isDual ? "Total size" : "Size"} labelFor="size-val" style={{ flex: 1 }}>
                                <InputGroup
                                    id="size-val"
                                    type="number"
                                    min={1}
                                    value={String(sizeValue)}
                                    onChange={e => setSizeValue(Number(e.target.value))}
                                    onKeyDown={handleAdvanceFromInput}
                                />
                            </FormGroup>
                            <FormGroup label="Unit" labelFor="size-unit">
                                <HTMLSelect
                                    id="size-unit"
                                    value={sizeUnit}
                                    onChange={e => setSizeUnit(e.target.value as SizeUnit)}
                                    onKeyDown={handleAdvanceFromInput}
                                    options={[
                                        { value: "M", label: "MB" },
                                        { value: "G", label: "GB" },
                                        { value: "T", label: "TB" },
                                    ]}
                                />
                            </FormGroup>
                        </div>}
                        {isDual && (
                            <FormGroup label={`Second volume size (${sizeUnit === "M" ? "MB" : sizeUnit === "G" ? "GB" : "TB"})`} labelFor="second-size-val" helperText="Same unit as the total size, and must be smaller — the gap holds plausible cover files.">
                                <InputGroup
                                    id="second-size-val"
                                    type="number"
                                    min={1}
                                    value={String(secondSizeValue)}
                                    onChange={e => setSecondSizeValue(Number(e.target.value))}
                                    onKeyDown={handleAdvanceFromInput}
                                />
                            </FormGroup>
                        )}
                        {isAdvanced && !isDual && (
                            <div className="quick-format-row">
                                <label className="quick-toggle">
                                    <CheckboxControl checked={quickFormat} ariaLabel="Quick format" onChange={event => setQuickFormat(event.currentTarget.checked)} />
                                    <span>Quick format</span>
                                    <span className="quick-desc">Faster but marginally less secure. Recommended for most use cases.</span>
                                </label>
                                {targetKind === "file" && (
                                    <label className="quick-toggle">
                                        <CheckboxControl checked={dynamicFormat} ariaLabel="Dynamic container" onChange={event => { setDynamicFormat(event.currentTarget.checked); if (event.currentTarget.checked) setQuickFormat(true); }} />
                                        <span>Dynamic container</span>
                                        <span className="quick-desc">Sparse file that grows as data is written.</span>
                                    </label>
                                )}
                            </div>
                        )}
                    </div>
                );

            case "encryption":
                return (
                    <div className="wizard-step-content">
                        <p className="step-description">Select the encryption algorithm and key derivation hash. Defaults are suitable for all use cases.</p>
                        <FormGroup label="Algorithm" labelFor="enc-algo">
                            <HTMLSelect
                                id="enc-algo"
                                value={encAlgo}
                                onChange={e => setEncAlgo(e.target.value as EncAlgo)}
                                onKeyDown={handleAdvanceFromInput}
                                options={ENC_ALGOS.map(a => ({ value: a.value, label: a.label }))}
                                fill
                            />
                        </FormGroup>
                        <FormGroup label="Key Derivation" labelFor="hash-algo">
                            <HTMLSelect
                                id="hash-algo"
                                value={hashAlgo}
                                onChange={e => setHashAlgo(e.target.value as HashAlgo)}
                                onKeyDown={handleAdvanceFromInput}
                                options={HASH_ALGOS.map(h => ({ value: h.value, label: h.label }))}
                                fill
                            />
                        </FormGroup>
                    </div>
                );

            case "filesystem":
                return (
                    <div className="wizard-step-content">
                        <p className="step-description">Select the filesystem for the volume contents.</p>
                        <div className="fs-grid">
                            {FS_TYPES.map(f => (
                                <button
                                    type="button"
                                    key={f.value}
                                    className={`fs-card ${filesystem === f.value ? "selected" : ""}`}
                                    aria-pressed={filesystem === f.value}
                                    onClick={() => setFilesystem(f.value)}
                                >
                                    <strong>{f.label}</strong>
                                    <span>{f.desc}</span>
                                </button>
                            ))}
                        </div>
                    </div>
                );

            case "password":
                return (
                    <div className="wizard-step-content">
                        <p className="step-description">Set a strong password to protect your volume.</p>
                        <div className="info-callout" style={{ marginBottom: 16 }}>
                            <Icon icon="shield" intent="success" />
                            <span><strong>Passphrase guidance:</strong> Use a long, unique passphrase. 24+ characters is ideal, but any strong password above the minimum works.</span>
                        </div>
                        <FormGroup label={isDual ? "First password" : "Password"} labelFor="pw">
                            <InputGroup
                                id="pw"
                                type={showPassword ? "text" : "password"}
                                value={password}
                                autoComplete="new-password"
                                onChange={e => setPassword(e.target.value)}
                                onKeyDown={handleAdvanceFromInput}
                                rightElement={
                                    <Button icon={showPassword ? "eye-off" : "eye-open"} minimal aria-label={showPassword ? "Hide new volume password" : "Show new volume password"} onClick={() => setShowPassword(p => !p)} />
                                }
                            />
                            {password && (
                                <div className="pw-strength">
                                    <div className="pw-strength-bar">
                                        {[1, 2, 3, 4, 5, 6].map(i => (
                                            <div
                                                key={i}
                                                className="pw-bar-segment"
                                                style={{ background: i <= pwStrength.score ? pwStrength.color : "var(--color-border)" }}
                                            />
                                        ))}
                                    </div>
                                    <span style={{ color: pwStrength.color }}>{pwStrength.label}</span>
                                </div>
                            )}
                        </FormGroup>
                        <FormGroup label="Confirm Password" labelFor="pw-confirm">
                            <InputGroup
                                id="pw-confirm"
                                type={showPassword ? "text" : "password"}
                                value={passwordConfirm}
                                autoComplete="new-password"
                                onChange={e => setPasswordConfirm(e.target.value)}
                                onKeyDown={handleAdvanceFromInput}
                            />
                            {passwordConfirm && password !== passwordConfirm && (
                                <div className="pw-mismatch" role="alert">Passwords do not match</div>
                            )}
                        </FormGroup>
                        {password.length > 0 && password.length < 8 && (
                            <div className="pw-mismatch" role="alert">Minimum 8 characters required</div>
                        )}
                        {isDual && (
                            <>
                                <FormGroup label="Second password" labelFor="second-pw" style={{ marginTop: 16 }} helperText="A different password — it opens the second (inner) volume.">
                                    <InputGroup
                                        id="second-pw"
                                        type={showPassword ? "text" : "password"}
                                        value={secondPassword}
                                        autoComplete="new-password"
                                        onChange={e => setSecondPassword(e.target.value)}
                                        onKeyDown={handleAdvanceFromInput}
                                    />
                                </FormGroup>
                                <FormGroup label="Confirm Second Password" labelFor="second-pw-confirm">
                                    <InputGroup
                                        id="second-pw-confirm"
                                        type={showPassword ? "text" : "password"}
                                        value={secondPasswordConfirm}
                                        autoComplete="new-password"
                                        onChange={e => setSecondPasswordConfirm(e.target.value)}
                                        onKeyDown={handleAdvanceFromInput}
                                    />
                                    {secondPasswordConfirm && secondPassword !== secondPasswordConfirm && (
                                        <div className="pw-mismatch" role="alert">The two passwords do not match</div>
                                    )}
                                    {secondPassword.length > 0 && secondPassword === password && (
                                        <div className="pw-mismatch" role="alert">The second password must differ from the first</div>
                                    )}
                                </FormGroup>
                                <FormGroup label="Hidden-volume keyfile (optional)" labelFor="second-keyfile">
                                    <InputGroup id="second-keyfile" placeholder="Path to keyfile or folder" value={secondKeyfile} autoComplete="off" onChange={event => setSecondKeyfile(event.target.value)} rightElement={<Button icon="folder-open" minimal aria-label="Browse for hidden-volume keyfile" onClick={handleBrowseSecondKeyfile} />} />
                                </FormGroup>
                                <FormGroup label="Hidden-volume PIM (optional)" labelFor="second-pim">
                                    <InputGroup id="second-pim" type="number" min={1} max={MAX_PIM} placeholder="Default" value={secondPim} autoComplete="off" onChange={event => setSecondPim(event.target.value)} />
                                </FormGroup>
                            </>
                        )}
                        <>
                        <FormGroup label={isDual ? "Outer-volume keyfile (optional)" : "Keyfile (optional)"} labelFor="create-keyfile" style={{ marginTop: 16 }}>
                            <InputGroup
                                id="create-keyfile"
                                placeholder="Path to keyfile or folder"
                                value={keyfile}
                                autoComplete="off"
                                onChange={e => setKeyfile(e.target.value)}
                                onKeyDown={handleAdvanceFromInput}
                                rightElement={
                                    <Button icon="folder-open" minimal aria-label="Browse for a new volume keyfile" onClick={handleBrowseKeyfile} />
                                }
                            />
                        </FormGroup>
                        <FormGroup label={isDual ? "Outer-volume PIM (optional)" : "PIM (optional)"} labelFor="create-pim" helperText={`Leave blank for default. PIM below ${hashAlgo === "argon2id" ? 12 : 485} requires a password of at least 20 UTF-8 bytes.`}>
                            <InputGroup
                                id="create-pim"
                                type="number"
                                min={1}
                                max={MAX_PIM}
                                placeholder="Default"
                                value={pim}
                                autoComplete="off"
                                onChange={e => setPim(e.target.value)}
                                onKeyDown={handleAdvanceFromInput}
                            />
                        </FormGroup>
                        </>
                    </div>
                );

            case "summary":
                if (applyLog.length > 0 || applying || done) {
                    return (
                        <div className="wizard-step-content">
                            <div className="apply-log">
                                {applyLog.map((item, i) => (
                                    <div key={i} className={`apply-log-item ${item.status}`}>
                                        <span className="apply-log-icon">
                                            {item.status === "ok" && <Icon icon="tick-circle" intent="success" />}
                                            {item.status === "error" && <Icon icon="error" intent="danger" />}
                                            {item.status === "running" && <span className="spinner-sm" />}
                                            {item.status === "pending" && <Icon icon="circle" />}
                                        </span>
                                        <div className="apply-log-text">
                                            <span className="apply-log-label">{item.label}</span>
                                            {item.detail && <span className="apply-log-detail">{item.detail}</span>}
                                        </div>
                                    </div>
                                ))}
                                {done && (
                                    <div className="apply-done-msg">
                                        <Icon icon="lock" />
                                        Volume created successfully. You can now mount it.
                                    </div>
                                )}
                                {hasError && !done && (
                                    <div className="apply-error-msg">
                                        <Icon icon="warning-sign" intent="warning" />
                                        <span>An error occurred. Press <strong>Back</strong> to adjust settings and retry, or <strong>Cancel</strong> to close.</span>
                                    </div>
                                )}
                            </div>
                        </div>
                    );
                }
                return (
                    <div className="wizard-step-content">
                        <p className="step-description">Review your configuration before creating the volume.</p>
                        <div className="summary-table">
                            <div className="summary-row"><span>Type</span><span>{isDual ? "Hidden + decoy" : "Standard"}</span></div>
                            <div className="summary-row"><span>Host</span><span>{targetKind === "device" ? "Entire partition" : "File container"}</span></div>
                            <div className="summary-row"><span>Location</span><span className="summary-path">{targetKind === "device" ? selectedPartition?.devicePath : volumePath}</span></div>
                            <div className="summary-row"><span>{isDual ? "Outer size" : "Size"}</span><span>{targetKind === "device" ? selectedPartition?.size : `${sizeValue}${sizeUnit}B`}</span></div>
                            {isDual && <div className="summary-row"><span>Hidden size</span><span>{secondSizeValue}{sizeUnit}B</span></div>}
                            {isAdvanced && <div className="summary-row"><span>Algorithm</span><span>{encAlgo}</span></div>}
                            {isAdvanced && <div className="summary-row"><span>Key Derivation</span><span>{hashAlgo.toUpperCase()}</span></div>}
                            <div className="summary-row"><span>Filesystem</span><span>{filesystem}</span></div>
                            {!isDual && <div className="summary-row"><span>Format Mode</span><span>{dynamicFormat ? "Dynamic" : quickFormat ? "Quick" : "Full"}</span></div>}
                            {keyfile && <div className="summary-row"><span>{isDual ? "Outer keyfile" : "Keyfile"}</span><span className="summary-path">{keyfile}</span></div>}
                            {secondKeyfile && <div className="summary-row"><span>Hidden keyfile</span><span className="summary-path">{secondKeyfile}</span></div>}
                            {pim && <div className="summary-row"><span>{isDual ? "Outer PIM" : "PIM"}</span><span>{pim}</span></div>}
                            {secondPim && <div className="summary-row"><span>Hidden PIM</span><span>{secondPim}</span></div>}
                        </div>
                    </div>
                );
            default:
                return null;
        }
    };

    const isSummary = currentStep?.id === "summary";
    const handleAdvanceFromInput = useCallback((event: React.KeyboardEvent<HTMLElement>) => {
        if (event.key !== "Enter" || event.shiftKey || event.defaultPrevented) return;
        event.preventDefault();

        if (done) {
            handleClose();
            return;
        }

        if (isSummary) {
            if (!applying) {
                void handleCreate();
            }
            return;
        }

        if (canNext) {
            setStepIndex((index) => index + 1);
        }
    }, [applying, canNext, done, handleClose, handleCreate, isSummary]);
    const handleWizardEnter = useCallback((event: React.KeyboardEvent<HTMLDivElement>) => {
        if (event.key !== "Enter" || event.shiftKey || event.defaultPrevented) return;

        const target = event.target as HTMLElement | null;
        if (!target) return;
        if (target.closest("button")) return;
        if (target instanceof HTMLTextAreaElement) return;

        event.preventDefault();

        if (done) {
            handleClose();
            return;
        }

        if (isSummary) {
            if (!applying) {
                void handleCreate();
            }
            return;
        }

        if (canNext) {
            setStepIndex((index) => index + 1);
        }
    }, [applying, canNext, done, handleClose, handleCreate, isSummary]);

    return (
        <Dialog
            isOpen={isOpen}
            title="Create Encrypted Volume"
            onClose={(applying && !hasError) ? undefined : handleClose}
            canOutsideClickClose={!applying || hasError}
            canEscapeKeyClose={!applying || hasError}
            className={`wizard-dialog ${theme === "light" ? "light" : ""}`}
            backdropProps={{
                style: {
                    backgroundColor: theme === "light" ? "rgba(0,0,0,0.4)" : "rgba(0,0,0,0.75)",
                    backdropFilter: "blur(4px)",
                },
            }}
        >
            <div className="wizard-layout" onKeyDown={handleWizardEnter}>
                {/* Step nav sidebar */}
                <div className="wizard-nav">
                    <div className="wizard-nav-title">NEW VOLUME</div>
                    {steps.map((step, i) => (
                        <div
                            key={step.id}
                            className={`wizard-nav-item ${i === stepIndex ? "active" : ""} ${i < stepIndex ? "done" : ""}`}
                            onClick={() => { if (!applying && i < stepIndex) setStepIndex(i); }}
                        >
                            <span className="wizard-nav-num">
                                {i < stepIndex ? <Icon icon="tick" size={10} /> : String(i + 1).padStart(2, "0")}
                            </span>
                            <span>{step.title}</span>
                        </div>
                    ))}
                </div>

                {/* Main content */}
                <div className="wizard-main">
                    <div className="wizard-step-title">{currentStep?.title}</div>
                    <div className="wizard-body">{renderStepContent()}</div>
                    <div className="wizard-footer">
                        <Button
                            text="CANCEL"
                            minimal
                            className="modal-cancel-btn"
                            disabled={applying && !hasError}
                            onClick={handleClose}
                        />
                        <div style={{ display: "flex", gap: 8 }}>
                            {stepIndex > 0 && !done && (
                                <Button
                                    icon="arrow-left"
                                    text="BACK"
                                    minimal
                                    className="modal-cancel-btn"
                                    disabled={applying && !hasError}
                                    onClick={() => { if (hasError) { setApplyLog([]); setApplying(false); } setStepIndex(i => i - 1); }}
                                />
                            )}
                            {!isSummary && (
                                <Button
                                    rightIcon="arrow-right"
                                    text="NEXT"
                                    className="modal-primary-btn"
                                    disabled={!canNext}
                                    onClick={() => setStepIndex(i => i + 1)}
                                />
                            )}
                            {isSummary && !done && (
                                <Button
                                    icon="lock"
                                    text={applying ? "CREATING..." : "CREATE VOLUME"}
                                    className="modal-primary-btn"
                                    loading={applying}
                                    disabled={applying}
                                    onClick={handleCreate}
                                />
                            )}
                            {done && (
                                <Button
                                    icon="tick"
                                    text="DONE"
                                    className="modal-primary-btn"
                                    onClick={handleClose}
                                />
                            )}
                        </div>
                    </div>
                </div>
            </div>
        </Dialog>
    );
}

export default CreateVolumeWizard;
