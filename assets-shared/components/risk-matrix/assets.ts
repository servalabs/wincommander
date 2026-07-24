// Self-contained asset resolver for the RiskMatrix component.
//
// Because this file lives INSIDE the shared assets repo, the relative paths
// below resolve identically no matter where the repo is mounted as a submodule
// (servalabs.com `assets/…`, wincommander `assets-shared/…`). Each consuming
// app's Vite build fingerprints these imports to final URLs. Hosts therefore
// inject NOTHING — the component owns its own imagery.

// — entity logos (companies, agencies, flags) —
import googleLogo from "../../entities/google-logo.svg";
import microsoftLogo from "../../entities/microsoft-logo.svg";
import appleLogo from "../../entities/apple-logo.svg";
import openaiLogo from "../../entities/openai-logo.svg";
import nvidiaLogo from "../../entities/nvidia-logo.svg";
import metaLogo from "../../entities/meta-logo.png";
import amazonLogo from "../../entities/amazon-logo.svg";
import zoomLogo from "../../entities/zoom-logo.svg";
import oracleLogo from "../../entities/oracle-logo.svg";
import adobeLogo from "../../entities/adobe-logo.png";
import slackLogo from "../../entities/slack-logo.svg";
import nsaLogo from "../../entities/nsa-logo.png";
import ciaLogo from "../../entities/cia-logo.png";
import fbiLogo from "../../entities/fbi-seal.svg";
import rsaLogo from "../../entities/rsa-logo.svg";
import juniperLogo from "../../entities/juniper-logo.svg";
import ciscoLogo from "../../entities/cisco-logo.svg";
import usaFlag from "../../entities/usa-flag.jpg";
import indiaFlag from "../../entities/india-flag.png";

// — editorial imagery —
import googleAd1999 from "../../defame/google-ad-1999.jpeg";
import gemini from "../../defame/gemini.png";
import humansReviewGemini from "../../defame/humans-review-gemini.jpg";
import metaRayban from "../../defame/meta-rayban-scandal.jpg";
import nsaPrismSlide from "../../defame/nsa-prism-slide.png";
import chatgptNsaDirector from "../../defame/chatgpt-nsa-director.jpg";
import clipperChip from "../../defame/myk-78-clipper-chip-markings.jpg";
import antDropoutjeep from "../../defame/nsa-ant-dropoutjeep.jpg";
import antCottonmouth from "../../defame/nsa-ant-cottonmouth-i.jpg";
import muscularSlide from "../../defame/nsa-muscular-google-cloud-slide.jpg";
import boundlessInformant from "../../defame/boundless-informant-heat-map.svg";
import merkelPortrait from "../../defame/angela-merkel-portrait-2011.jpg";
import utahDataCenter from "../../defame/nsa-utah-data-center-aerial.jpg";
import ciaFrankfurt from "../../defame/cia-frankfurt-consulate.jpg";
import natanz from "../../defame/natanz-nuclear-facility-2006.jpg";
import antJetplow from "../../defame/nsa-ant-jetplow.jpg";
import haydenPortrait from "../../defame/michael-hayden-portrait.jpg";

// — fingerprint status icons —
import protonVpn from "../../softwares/proton-vpn.svg";
import tor from "../../softwares/tor.png";

/** Logos keyed by filename (matches the `logo` field in scandals.ts). */
export const logos: Record<string, string> = {
  "google-logo.svg": googleLogo,
  "microsoft-logo.svg": microsoftLogo,
  "apple-logo.svg": appleLogo,
  "openai-logo.svg": openaiLogo,
  "nvidia-logo.svg": nvidiaLogo,
  "meta-logo.png": metaLogo,
  "amazon-logo.svg": amazonLogo,
  "zoom-logo.svg": zoomLogo,
  "oracle-logo.svg": oracleLogo,
  "adobe-logo.png": adobeLogo,
  "slack-logo.svg": slackLogo,
  "nsa-logo.png": nsaLogo,
  "cia-logo.png": ciaLogo,
  "fbi-seal.svg": fbiLogo,
  "rsa-logo.svg": rsaLogo,
  "juniper-logo.svg": juniperLogo,
  "cisco-logo.svg": ciscoLogo,
  "usa-flag.jpg": usaFlag,
  "india-flag.png": indiaFlag,
};

/** Editorial images keyed by filename (matches the `image` field in events). */
export const editorial: Record<string, string> = {
  "google-ad-1999.jpeg": googleAd1999,
  "gemini.png": gemini,
  "humans-review-gemini.jpg": humansReviewGemini,
  "meta-rayban-scandal.jpg": metaRayban,
  "nsa-prism-slide.png": nsaPrismSlide,
  "chatgpt-nsa-director.jpg": chatgptNsaDirector,
  "myk-78-clipper-chip-markings.jpg": clipperChip,
  "nsa-ant-dropoutjeep.jpg": antDropoutjeep,
  "nsa-ant-cottonmouth-i.jpg": antCottonmouth,
  "nsa-muscular-google-cloud-slide.jpg": muscularSlide,
  "boundless-informant-heat-map.svg": boundlessInformant,
  "angela-merkel-portrait-2011.jpg": merkelPortrait,
  "nsa-utah-data-center-aerial.jpg": utahDataCenter,
  "cia-frankfurt-consulate.jpg": ciaFrankfurt,
  "natanz-nuclear-facility-2006.jpg": natanz,
  "nsa-ant-jetplow.jpg": antJetplow,
  "michael-hayden-portrait.jpg": haydenPortrait,
};

/** Icons used by the FingerprintMirror status tiles. */
export const fingerprintIcons = { vpn: protonVpn, tor } as const;

export const usaFlagUrl = usaFlag;
export const indiaFlagUrl = indiaFlag;
