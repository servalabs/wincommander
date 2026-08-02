import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import SectionCard from "../../components/shared/SectionCard";
import { Button, FormGroup, InputGroup } from "@/components/ui/bp";
import { useAppState } from "../../context/AppContext";
import useBackend from "../../hooks/useBackend";
import { showError, showSuccess } from "../../utils/toast";

export default function BrandingLicensingSection() {
    const { appSettings, patchAppSettings, refreshHardening } = useAppState();
    const { setOEMInformation, setAppBranding } = useBackend();

    const [computerName, setComputerNameInput] = useState("SovereignOS");
    const [manufacturer, setManufacturer] = useState("ServaLabs");
    const [supportUrl, setSupportUrl] = useState("https://servalabs.com");
    const [supportProvider, setSupportProvider] = useState("ServaLabs Support");
    const [logoPath, setLogoPath] = useState("");
    const [appCompany, setAppCompany] = useState("ServaLabs");
    const [appProduct, setAppProduct] = useState("WinCommander");
    const [loadingMap, setLoadingMap] = useState<Record<string, boolean>>({});

    useEffect(() => {
        const branding = appSettings?.ideal?.identity?.branding;
        if (!branding) return;
        if (branding.pcName) setComputerNameInput(branding.pcName);
        if (branding.manufacturer) setManufacturer(branding.manufacturer);
        if (branding.supportUrl) {
            setSupportUrl(branding.supportUrl.includes("discord") ? "https://servalabs.com" : branding.supportUrl);
        } else {
            setSupportUrl("https://servalabs.com");
        }
        if (branding.companyName) setAppCompany(branding.companyName);
        if (branding.productName) setAppProduct(branding.productName);
    }, [appSettings]);

    const runWithLoading = async (key: string, fn: () => Promise<void>) => {
        setLoadingMap(prev => ({ ...prev, [key]: true }));
        try {
            await fn();
        } finally {
            setLoadingMap(prev => ({ ...prev, [key]: false }));
        }
    };

    const handleBrowseLogo = async () => {
        try {
            const selected = await open({
                multiple: false,
                filters: [{ name: "Bitmap Logo", extensions: ["bmp"] }],
            });
            if (selected && typeof selected === "string") setLogoPath(selected);
        } catch (err) {
            console.error("Failed to open file picker", err);
        }
    };

    const handleRename = async () => {
        await runWithLoading("rename", async () => {
            const result = await setOEMInformation(computerName, manufacturer, supportUrl, supportProvider, logoPath);
            if (result.success && result.data) {
                showSuccess("OEM Information updated successfully!");
                await refreshHardening();
                await patchAppSettings({
                    ideal: {
                        identity: {
                            branding: { pcName: computerName, manufacturer, supportUrl },
                        },
                    },
                }).catch(() => {});
            } else if (!result.success) {
                showError(result.error || "Failed to update OEM information.");
            }
        });
    };

    const handleApplyAppBranding = async () => {
        await runWithLoading("appBranding", async () => {
            const result = await setAppBranding(appCompany, appProduct);
            if (result.success) {
                await patchAppSettings({
                    ideal: {
                        identity: {
                            branding: {
                                companyName: appCompany.trim() || "ServaLabs",
                                productName: appProduct.trim() || "WinCommander",
                            },
                        },
                    },
                }).catch(() => {});
                showSuccess("App branding updated. Title bar now shows the new name.");
            } else {
                showError(result.error || "Failed to apply app branding.");
            }
        });
    };

    return (
        <SectionCard title="OS Personalization & App Whitelabeling" icon="id-number">
            <div className="secret-form-grid mb-5">
                <FormGroup label="OS Model Name" labelFor="os-model" className="compact-form">
                    <InputGroup id="os-model" placeholder="SovereignOS" value={computerName} onChange={(e) => setComputerNameInput(e.target.value)} />
                </FormGroup>
                <FormGroup label="Manufacturer" labelFor="manufacturer" className="compact-form">
                    <InputGroup id="manufacturer" placeholder="ServaLabs" value={manufacturer} onChange={(e) => setManufacturer(e.target.value)} />
                </FormGroup>
                <FormGroup label="Support Provider" labelFor="support" className="compact-form">
                    <InputGroup id="support" placeholder="ServaLabs Support" value={supportProvider} onChange={(e) => setSupportProvider(e.target.value)} />
                </FormGroup>
                <FormGroup label="Support URL" labelFor="url" className="compact-form">
                    <InputGroup id="url" placeholder="https://servalabs.com" value={supportUrl} onChange={(e) => setSupportUrl(e.target.value)} />
                </FormGroup>
            </div>
            <FormGroup label="OEM Logo Path (.bmp)" labelFor="logo-path" className="compact-form mb-5">
                <InputGroup
                    id="logo-path"
                    placeholder="C:\\Path\\To\\Logo.bmp"
                    value={logoPath}
                    onChange={(e) => setLogoPath(e.target.value)}
                    rightElement={
                        <Button
                            icon="folder-open"
                            minimal
                            onClick={handleBrowseLogo}
                            aria-label="Browse for OEM logo"
                            title="Browse for OEM logo"
                        />
                    }
                />
            </FormGroup>
            <div className="flex justify-end">
                <Button text="Apply OEM Branding" icon="id-number" onClick={handleRename} loading={loadingMap.rename} className="compact-action-btn" />
            </div>

            <div className="sec-divider" />

            <div className="secret-form-grid mb-5">
                <FormGroup label="Company Name" className="compact-form">
                    <InputGroup placeholder="ServaLabs" value={appCompany} onChange={(e) => setAppCompany(e.target.value)} />
                </FormGroup>
                <FormGroup label="Product Name" className="compact-form">
                    <InputGroup placeholder="WinCommander" value={appProduct} onChange={(e) => setAppProduct(e.target.value)} />
                </FormGroup>
            </div>
            <div className="flex justify-end">
                <Button text="Apply App Branding" icon="edit" onClick={handleApplyAppBranding} loading={loadingMap.appBranding} className="compact-action-btn" />
            </div>
        </SectionCard>
    );
}
