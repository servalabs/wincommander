import type { Severity, RiskSource } from "./types";

// Sovereignty Risk Matrix + Exposure Ledger — SINGLE SOURCE OF TRUTH.
//
// This file (exposures.ts) is the SSOT for the Sovereignty Risk Matrix and
// the headline-driven exposure ledger shared by servalabs.com and the
// WinCommander dashboard. `scandals.ts` derives its `SCANDALS` map — the
// data structure actually consumed by the rendered Risk Matrix component —
// from `EXPOSURES` (the punchy-headline event ledger) and `ENTITIES` (the
// per-node metadata) defined below. `SOURCES.md` remains the citation
// verification ledger: every claim behind every `explanation` below was
// checked against the authoritative sources listed in `sources`, quotes and
// verification notes live there, and wording here is tightened to what
// those sources support.
//
// EXPOSURES entries are grouped by entity in the same order as scandals.ts
// (nvidia, google, microsoft, meta, amazon, apple, openai, zoom, oracle,
// adobe, slack, nsa, usgov, cia, fbi, rsa, juniper, cisco) so a reader can
// scan company-by-company. 18 entities, 111 events — one Exposure per event,
// none dropped, merged, or skipped.

export interface EntityMeta {
  name: string;
  /** Logo key resolved via the bundled asset map (e.g. "google-logo.svg"). */
  logo: string;
  /** Node/brand accent color (any CSS color). */
  color: string;
  /** Short editorial tagline shown under the name, e.g. "The Data Harvester". */
  description: string;
  /** Placement: orbiting tech giant, or a central intel/state node. */
  category: "tech" | "agency";
}

export interface Exposure {
  /** The ENTITIES key this event belongs to, e.g. "google", "meta", "nsa". */
  entity: string;
  /** Punchy, consumer-legible headline — the point of this ledger. */
  headline: string;
  /** Brief (1-3 sentence) explanation of what happened. */
  explanation: string;
  year: string;
  severity: Severity;
  /** Capped at the 4 strongest citations. */
  sources: RiskSource[];
  /** Editorial image key, resolved via the bundled asset map (optional). */
  image?: string;
}

export const ENTITIES: Record<string, EntityMeta> = {
  nvidia: {
    name: "NVIDIA",
    logo: "nvidia-logo.svg",
    color: "#76B900",
    description: "The Hardware Choke",
    category: "tech",
  },
  google: {
    name: "Google",
    logo: "google-logo.svg",
    color: "#EA4335",
    description: "The Data Harvester",
    category: "tech",
  },
  microsoft: {
    name: "Microsoft",
    logo: "microsoft-logo.svg",
    color: "#00A4EF",
    description: "The Corporate Spy",
    category: "tech",
  },
  meta: {
    name: "Meta",
    logo: "meta-logo.png",
    color: "#1877F2",
    description: "The Election Rigger",
    category: "tech",
  },
  amazon: {
    name: "Amazon",
    logo: "amazon-logo.svg",
    color: "#FF9900",
    description: "The Home Invader",
    category: "tech",
  },
  apple: {
    name: "Apple",
    logo: "apple-logo.svg",
    color: "#A2AAAD",
    description: "The Privacy 'Wolf'",
    category: "tech",
  },
  openai: {
    name: "OpenAI",
    logo: "openai-logo.svg",
    color: "#10A37F",
    description: "The Trojan Horse",
    category: "tech",
  },
  zoom: {
    name: "Zoom",
    logo: "zoom-logo.svg",
    color: "#2D8CFF",
    description: "The China Relay",
    category: "tech",
  },
  oracle: {
    name: "Oracle",
    logo: "oracle-logo.svg",
    color: "#F80000",
    description: "The Defense Contractor",
    category: "tech",
  },
  adobe: {
    name: "Adobe",
    logo: "adobe-logo.png",
    color: "#FF0000",
    description: "The Asset Seizure",
    category: "tech",
  },
  slack: {
    name: "Slack",
    logo: "slack-logo.svg",
    color: "#4A154B",
    description: "The Identity Purge",
    category: "tech",
  },
  nsa: {
    name: "NSA",
    logo: "nsa-logo.png",
    color: "#e5e7eb",
    description: "The Global Ear",
    category: "agency",
  },
  usgov: {
    name: "US Government",
    logo: "usa-flag.jpg",
    color: "#dc2626",
    description: "The Legal Architect",
    category: "agency",
  },
  cia: {
    name: "CIA",
    logo: "cia-logo.png",
    color: "#e5e7eb",
    description: "The Cyber Weaponizer",
    category: "agency",
  },
  fbi: {
    name: "FBI",
    logo: "fbi-seal.svg",
    color: "#002E6D",
    description: "The Domestic Operator",
    category: "agency",
  },
  rsa: {
    name: "RSA",
    logo: "rsa-logo.svg",
    color: "#CC0000",
    description: "The Bought Backdoor",
    category: "tech",
  },
  juniper: {
    name: "Juniper Networks",
    logo: "juniper-logo.svg",
    color: "#84B135",
    description: "The Unlocked Backdoor",
    category: "tech",
  },
  cisco: {
    name: "Cisco",
    logo: "cisco-logo.svg",
    color: "#049FD9",
    description: "The Intercepted Router",
    category: "tech",
  },
};

export const EXPOSURES: Exposure[] = [
  // ---- NVIDIA ----
  {
    entity: "nvidia",
    headline: "Washington turned Nvidia's AI chips into a bargaining chip with China",
    explanation:
      "Since October 2022 the US has restricted Nvidia's advanced AI chip exports to China as a policy lever, tightening controls through 2023-2024 before the Trump administration allowed conditional H200 sales in December 2025.",
    year: "2022–25",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.cfr.org/expert-brief/consequences-exporting-nvidias-h200-chips-china", label: "Council on Foreign Relations" },
      { url: "https://www.congress.gov/crs-product/R48642", label: "Congressional Research Service" },
    ],
  },
  {
    entity: "nvidia",
    headline: "Even gaming GPUs got swept into US-China chip export bans",
    explanation:
      "New licensing rules effective November 17, 2023 extended US export controls to Nvidia's consumer RTX 4090 GPU for China, Saudi Arabia, the UAE, and Vietnam, because it shares silicon with restricted data-center chips.",
    year: "2023",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.tomshardware.com/pc-components/gpus/nvidia-rtx-4090-subject-to-china-export-restrictions-starting-november-17", label: "Tom's Hardware" },
      { url: "https://videocardz.com/newz/u-s-to-restrict-shipment-of-nvidia-h800-and-rtx-4090-gpus-to-china", label: "VideoCardz" },
    ],
  },
  {
    entity: "nvidia",
    headline: "The US ordered Nvidia to stop shipping AI chips to China overnight",
    explanation:
      "On October 17, 2023 the Commerce Department banned sales of Nvidia's A100, H100, and China-tailored A800/H800 chips to China, then moved up enforcement to October 23 for immediate cessation of exports.",
    year: "2023",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.cnbc.com/2023/10/17/us-bans-export-of-more-ai-chips-including-nvidia-h800-to-china.html", label: "CNBC" },
      { url: "https://technode.com/2023/10/26/us-tells-nvidia-to-immediately-cease-ai-chip-exports-to-china/", label: "TechNode" },
    ],
  },

  // ---- Google ----
  {
    entity: "google",
    headline: "Google once promised a search engine with zero ads, ever",
    explanation:
      "A 1999 Google ad marketed its search engine as 'pure' — no ads, no sponsor links, no portal clutter — years before Google built the world's largest digital ad business.",
    year: "1999",
    severity: "CRITICAL",
    sources: [
      { url: "https://cybercultural.com/p/google-1999/", label: "Cybercultural" },
      { url: "https://www.uniladtech.com/news/tech-news/google-ad-from-1999-588164-20240314", label: "UNILAD Tech" },
    ],
    image: "google-ad-1999.jpeg",
  },
  {
    entity: "google",
    headline: "Google scanned your Gmail to sell ads for over a decade",
    explanation:
      "Advertising made up roughly 77% of Alphabet's 2023 revenue ($237.85B of $307.39B). Google scanned Gmail content to personalize ads until it discontinued the practice for consumer accounts in 2017.",
    year: "Ongoing",
    severity: "CRITICAL",
    sources: [
      { url: "https://fourweekmba.com/google-revenue-breakdown/", label: "FourWeekMBA" },
      { url: "https://www.npr.org/sections/thetwo-way/2017/06/26/534451513/google-says-it-will-no-longer-read-users-emails-to-sell-targeted-ads", label: "NPR" },
      { url: "https://time.com/4831200/google-gmail-ads-advertising-email/", label: "TIME" },
    ],
  },
  {
    entity: "google",
    headline: "Your phone phones home every four and a half minutes, even idle",
    explanation:
      "A March 2021 Trinity College Dublin study found Android and iOS phones transmit data to their platform makers every 4.5 minutes on average, even sitting idle — Pixel devices sent about 1MB every 12 hours idle versus 52KB from the iPhone tested.",
    year: "2021",
    severity: "HIGH",
    sources: [
      { url: "https://www.irishtimes.com/business/technology/smartphones-share-our-data-every-four-and-a-half-minutes-says-study-1.4521267", label: "The Irish Times" },
    ],
  },
  {
    entity: "google",
    headline: "Humans at Google read your Gemini chats",
    explanation:
      "Google discloses that trained human reviewers may read and annotate Gemini Apps conversations to improve the product, retaining disconnected chat data for up to three years — and warns users not to type anything they wouldn't want a reviewer to see.",
    year: "Ongoing",
    severity: "HIGH",
    sources: [
      { url: "https://support.google.com/gemini/answer/13594961?hl=en", label: "Google" },
    ],
    image: "humans-review-gemini.jpg",
  },
  {
    entity: "google",
    headline: "One US order cut Huawei's phones off Gmail, Maps, and the Play Store",
    explanation:
      "After the Commerce Department placed Huawei on its Entity List in May 2019, Google was barred from licensing Google Mobile Services to Huawei, cutting new devices off from the Play Store, Gmail, Maps, and YouTube.",
    year: "2019",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.androidauthority.com/huawei-google-android-ban-988382/", label: "Android Authority" },
    ],
  },
  {
    entity: "google",
    headline: "Google paid Texas $1.375 billion over secretly tracked faces and locations",
    explanation:
      "Google agreed to pay Texas $1.375 billion, finalized October 2025, resolving claims it collected location data and Incognito browsing activity, plus biometric identifiers like voiceprints and facial geometry via Photos, Assistant, and Nest Hub Max — all without consent.",
    year: "2025",
    severity: "HIGH",
    sources: [
      { url: "https://www.texasattorneygeneral.gov/news/releases/attorney-general-ken-paxton-finalizes-historic-settlement-google-and-secures-1375-billion-big-tech", label: "Texas Attorney General" },
      { url: "https://www.bracewell.com/resources/google-agrees-to-1-375-billion-settlement-as-texas-attorney-general-continues-data-privacy-push/", label: "Bracewell LLP" },
    ],
  },
  {
    entity: "google",
    headline: "Google's 'Incognito' mode was tracking you the whole time",
    explanation:
      "Google agreed in April 2024 to delete or de-identify billions of records of users' private 'Incognito mode' browsing and block third-party cookies in Incognito for five years, with no direct payout to the roughly 136 million affected users.",
    year: "2024",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.npr.org/2024/04/01/1242019127/google-incognito-mode-settlement-search-history", label: "NPR" },
      { url: "https://thehackernews.com/2024/04/google-to-delete-billions-of-browsing.html", label: "The Hacker News" },
    ],
  },
  {
    entity: "google",
    headline: "Google permanently deleted two dads' accounts over medical photos of their kids — police cleared them, Google didn't restore them",
    explanation:
      "Google's automated CSAM scanner flagged medical photos two fathers took of their toddlers at a doctor's request, leading Google to permanently disable their accounts. Police investigations found no crime occurred in either case, but Google never restored the accounts.",
    year: "2021–22",
    severity: "CRITICAL",
    sources: [
      { url: "https://gizmodo.com/google-csam-photodna-1849440471", label: "Gizmodo" },
    ],
  },
  {
    entity: "google",
    headline: "Turning off 'Location History' didn't actually stop Google from tracking you",
    explanation:
      "Google paid $391.5 million to 40 state attorneys general in a November 2022 settlement resolving claims it kept tracking users' location via 'Web & App Activity' even after they disabled 'Location History,' misleading users from 2014 to 2020.",
    year: "2022",
    severity: "HIGH",
    sources: [
      { url: "https://fortune.com/2022/11/14/google-settles-with-40-states-391-million-location-data-tracking-privacy/", label: "Fortune" },
      { url: "https://www.hunton.com/privacy-and-cybersecurity-law-blog/google-agrees-to-391-5-million-settlement-with-40-states-over-misleading-location-tracking-practices", label: "Hunton Andrews Kurth" },
    ],
  },

  // ---- Microsoft ----
  {
    entity: "microsoft",
    headline: "Microsoft, Amazon, Google and Oracle split a $9 billion Pentagon cloud deal",
    explanation:
      "Microsoft won the Pentagon's $10 billion JEDI cloud contract in 2019; after Amazon's legal challenge forced its cancellation, the Pentagon replaced it with the multi-vendor JWCC, splitting up to $9 billion between Microsoft, Amazon, Google, and Oracle in December 2022.",
    year: "2019/2022",
    severity: "HIGH",
    sources: [
      { url: "https://en.wikipedia.org/wiki/Joint_Enterprise_Defense_Infrastructure", label: "Wikipedia" },
      { url: "https://www.cnbc.com/2022/12/07/google-oracle-amazon-and-microsoft-awarded-9-billion-pentagon-cloud-deals.html", label: "CNBC" },
    ],
  },
  {
    entity: "microsoft",
    headline: "Windows sends 'required' diagnostic data to Microsoft — and you can't turn it off",
    explanation:
      "Windows collects 'required' diagnostic data by default that consumer editions cannot fully turn off, plus additional 'optional' data on browsing and app usage that users can disable — both categories are sent to Microsoft.",
    year: "Ongoing",
    severity: "HIGH",
    sources: [
      { url: "https://learn.microsoft.com/en-us/windows/privacy/optional-diagnostic-data", label: "Microsoft Learn" },
      { url: "https://learn.microsoft.com/en-us/windows/privacy/configure-windows-diagnostic-data-in-your-organization", label: "Microsoft Learn" },
    ],
  },
  {
    entity: "microsoft",
    headline: "Windows' 'Recall' quietly screenshotted everything you did — passwords and card numbers included",
    explanation:
      "Security researchers found Recall's screenshot database, including credit card and Social Security numbers even with the sensitive-info filter on, could be read as a normal user process. Microsoft delayed release and redesigned it to be opt-in and encrypted after backlash.",
    year: "2024",
    severity: "CRITICAL",
    sources: [
      { url: "https://thehackernews.com/2024/06/microsoft-revamps-controversial-ai.html", label: "The Hacker News" },
      { url: "https://securityboulevard.com/2024/11/microsofts-controversial-recall-feature-release-delayed-again/", label: "Security Boulevard" },
    ],
  },
  {
    entity: "microsoft",
    headline: "A stolen Microsoft key let Chinese hackers read US government email",
    explanation:
      "In 2023 the China-linked group Storm-0558 used a stolen Microsoft signing key to forge tokens and read email from roughly 25 organizations, including US government agencies. A federal review board called the intrusion preventable and Microsoft's security culture inadequate.",
    year: "2023",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.helpnetsecurity.com/2024/04/03/microsoft-storm-0558-key/", label: "Help Net Security" },
      { url: "https://www.microsoft.com/en-us/security/blog/2023/07/14/analysis-of-storm-0558-techniques-for-unauthorized-email-access/", label: "Microsoft Security Response Center Blog" },
    ],
  },
  {
    entity: "microsoft",
    headline: "Microsoft cut off product sales to Russia after the Ukraine invasion",
    explanation:
      "On March 4, 2022, following Russia's invasion of Ukraine, Microsoft announced it would suspend all new product and service sales in Russia, coordinated with US, EU, and UK sanctions.",
    year: "2022",
    severity: "HIGH",
    sources: [
      { url: "https://blogs.microsoft.com/on-the-issues/2022/03/04/microsoft-suspends-russia-sales-ukraine-conflict/", label: "Microsoft" },
      { url: "https://techcrunch.com/2022/03/10/amazon-microsoft-and-google-have-suspended-cloud-sales-in-russia/", label: "TechCrunch" },
    ],
  },
  {
    entity: "microsoft",
    headline: "GitHub locked developers in Iran and Syria out of private repos",
    explanation:
      "In July 2019 Microsoft-owned GitHub restricted private repositories, the Marketplace, and paid private organizations for developers in Iran, Syria, and Crimea, citing US export law; public repositories stayed accessible.",
    year: "2019",
    severity: "HIGH",
    sources: [
      { url: "https://techcrunch.com/2019/07/29/github-ban-sanctioned-countries/", label: "TechCrunch" },
    ],
  },
  {
    entity: "microsoft",
    headline: "Microsoft was the first tech company the NSA plugged into PRISM",
    explanation:
      "Leaked NSA documents identified Microsoft as the first tech company to join PRISM, in September 2007, and reported it worked with the NSA on Outlook.com, SkyDrive, and Skype; Microsoft denied giving any government blanket or direct access.",
    year: "2013",
    severity: "HIGH",
    sources: [
      { url: "https://en.wikipedia.org/wiki/PRISM", label: "Wikipedia" },
      { url: "https://thenextweb.com/news/guardian-microsoft-cooperated-with-nsa-giving-access-to-skydrive-skype-and-outlook-com-data", label: "TheNextWeb" },
    ],
  },
  {
    entity: "microsoft",
    headline: "Microsoft cut off an Indian company's email and Teams over sanctions",
    explanation:
      "On July 22, 2025 Microsoft suspended Outlook and Teams access for India's Nayara Energy, 49% owned by Russia's Rosneft, citing EU sanctions, then restored service on July 30 hours before a Delhi High Court hearing.",
    year: "2025",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.theregister.com/2025/08/04/nayara_energy_microsoft_india/", label: "The Register" },
      { url: "https://www.datacenterdynamics.com/en/news/microsoft-cuts-off-cloud-services-to-rosneft-backed-nayara-energy/", label: "Data Center Dynamics" },
    ],
  },

  // ---- Meta ----
  {
    entity: "meta",
    headline: "97% of Meta's revenue comes from ads built on your data",
    explanation:
      "Per Meta's FY2025 10-K, advertising revenue was about $196.2B of $201.0B total revenue — roughly 97.5% — making Meta's business almost entirely dependent on ads built from user profiling.",
    year: "2025",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.sec.gov/Archives/edgar/data/0001326801/000162828026003942/meta-20251231.htm", label: "SEC EDGAR" },
      { url: "https://s21.q4cdn.com/399680738/files/doc_news/Meta-Reports-Fourth-Quarter-and-Full-Year-2025-Results-2026.pdf", label: "Meta Investor Relations" },
    ],
  },
  {
    entity: "meta",
    headline: "WhatsApp encrypts your messages but still collects metadata Meta can share",
    explanation:
      "WhatsApp message content is end-to-end encrypted and WhatsApp says it doesn't log who's messaging whom, but it still collects metadata — device and IP info, activity timing, call duration — and can share categories of it with other Meta companies.",
    year: "Ongoing",
    severity: "HIGH",
    sources: [
      { url: "https://faq.whatsapp.com/683043392411948/?locale=en_US", label: "WhatsApp Help Center" },
      { url: "https://www.whatsapp.com/legal/privacy-policy", label: "WhatsApp (Meta) Privacy Policy" },
    ],
  },
  {
    entity: "meta",
    headline: "Europe fined Meta a record €1.2 billion for illegal US data transfers",
    explanation:
      "Ireland's Data Protection Commission fined Meta a record €1.2 billion in May 2023 for unlawfully transferring EU user data to the US after the CJEU's Schrems II ruling — still the largest GDPR fine ever issued.",
    year: "2023",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.dataprotection.ie/en/news-media/press-releases/Data-Protection-Commission-announces-conclusion-of-inquiry-into-Meta-Ireland", label: "Data Protection Commission (Ireland)" },
      { url: "https://noyb.eu/en/edpb-decision-facebooks-eu-us-data-transfers-stop-transfers-fine-and-repatriation", label: "noyb.eu" },
    ],
  },
  {
    entity: "meta",
    headline: "Facebook let an app harvest millions of users' data for political profiling",
    explanation:
      "Facebook let a third-party app improperly harvest data on tens of millions of users, which was passed to Cambridge Analytica for political profiling. The FTC imposed a then-record $5 billion penalty in July 2019.",
    year: "2019",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.ftc.gov/news-events/news/press-releases/2019/07/ftc-imposes-5-billion-penalty-sweeping-new-privacy-restrictions-facebook", label: "Federal Trade Commission" },
      { url: "https://www.forbes.com/sites/mnunez/2019/07/24/ftcs-unprecedented-slap-fines-facebook-5-billion-forces-new-privacy-controls/", label: "Forbes" },
    ],
  },
  {
    entity: "meta",
    headline: "Meta's Ray-Ban glasses beamed users' private footage — including nudity and bathroom moments — to contractors in Nairobi",
    explanation:
      "A Swedish newspaper investigation found workers at Sama, a Nairobi-based contractor, reviewed sensitive footage from Ray-Ban Meta smart glasses — including nudity and bathroom moments — prompting a March 2026 US class-action lawsuit against Meta and Luxottica.",
    year: "2026",
    severity: "CRITICAL",
    sources: [
      { url: "https://techcrunch.com/2026/03/05/meta-sued-over-ai-smartglasses-privacy-concerns-after-workers-reviewed-nudity-sex-and-other-footage/", label: "TechCrunch" },
      { url: "https://www.euronews.com/next/2026/03/06/meta-faces-privacy-lawsuit-over-ai-smart-glasses", label: "Euronews" },
    ],
    image: "meta-rayban-scandal.jpg",
  },

  // ---- Amazon ----
  {
    entity: "amazon",
    headline: "A Ring employee watched thousands of customers' bathroom and bedroom videos",
    explanation:
      "The FTC found Ring allowed broad, poorly monitored employee access to customers' private videos — one employee viewed thousands of recordings of female users in bathrooms and bedrooms over several months. Ring settled for $5.8 million in refunds.",
    year: "2023",
    severity: "HIGH",
    sources: [
      { url: "https://www.eff.org/deeplinks/2023/06/ftc-forces-ring-take-user-privacy-seriously", label: "Electronic Frontier Foundation" },
    ],
  },
  {
    entity: "amazon",
    headline: "Amazon kept kids' Alexa voice recordings forever, despite parents' deletion requests",
    explanation:
      "The FTC and DOJ charged Amazon with violating COPPA by keeping children's Alexa voice recordings indefinitely and ignoring parents' deletion requests; Amazon paid a $25 million civil penalty to settle.",
    year: "2023",
    severity: "HIGH",
    sources: [
      { url: "https://www.foxbusiness.com/technology/amazon-agrees-25m-settlement-alexa-unlawfully-storing-childrens-voice-recordings-location-data", label: "Fox Business" },
    ],
  },
  {
    entity: "amazon",
    headline: "Ring gave police doorbell footage without a warrant 11 times in one year",
    explanation:
      "Amazon disclosed to Senator Ed Markey that Ring gave police video footage without a warrant or the owner's consent 11 times in 2022, calling each a 'good-faith' emergency call made at its own discretion.",
    year: "2022",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.eff.org/deeplinks/2022/07/ring-reveals-they-give-videos-police-without-user-consent-or-warrant", label: "Electronic Frontier Foundation" },
      { url: "https://www.theregister.com/2022/07/14/amazon_gave_police_unauthorized_doorbell/", label: "The Register" },
    ],
  },
  {
    entity: "amazon",
    headline: "The NSA director during the Snowden leaks joined Amazon's board",
    explanation:
      "Retired General Keith Alexander, NSA director from 2005-2014 and US Cyber Command commander from 2010-2014, joined Amazon's board in September 2020.",
    year: "2020",
    severity: "HIGH",
    sources: [
      { url: "https://techcrunch.com/2020/09/09/former-nsa-chief-general-keith-alexander-is-now-on-amazons-board/", label: "TechCrunch" },
      { url: "https://www.geekwire.com/2020/amazon-adds-former-nsa-u-s-cyber-command-leader-keith-alexander-board-director/", label: "GeekWire" },
    ],
  },
  {
    entity: "amazon",
    headline: "Amazon built the CIA's own private cloud for $600 million",
    explanation:
      "AWS won a roughly $600 million contract to build a private cloud (C2S) for the CIA and allied intelligence agencies; after IBM's protest, a federal court upheld AWS's award in October 2013.",
    year: "2013",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.washingtontechnology.com/2013/10/amazon-win-restarts-cia-cloud-contract/338279/", label: "Washington Technology" },
      { url: "https://www.theregister.com/2013/06/03/ibm_protests_aws_cia_cloud/", label: "The Register" },
      { url: "https://www.informationweek.com/it-infrastructure/amazon-again-beats-ibm-for-cia-cloud-contract", label: "InformationWeek" },
    ],
  },

  // ---- Apple ----
  {
    entity: "apple",
    headline: "A secret iPhone chip flaw let spyware infect phones with zero clicks",
    explanation:
      "Kaspersky disclosed in December 2023 that a zero-click iMessage exploit chain used a previously undocumented hardware feature in Apple's A12-A16 chips to bypass kernel memory protection on iPhones. Apple had quietly patched it in July 2023.",
    year: "2023",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.kaspersky.com/about/press-releases/kaspersky-discloses-iphone-hardware-feature-vital-in-operation-triangulation-case", label: "Kaspersky" },
      { url: "https://securelist.com/operation-triangulation-the-last-hardware-mystery/111669/", label: "Kaspersky Securelist" },
      { url: "https://www.bleepingcomputer.com/news/security/iphone-triangulation-attack-abused-undocumented-hardware-feature/", label: "BleepingComputer" },
    ],
  },
  {
    entity: "apple",
    headline: "Apple paid $95 million after Siri recorded private conversations by accident",
    explanation:
      "Apple reached a $95 million settlement over claims Siri was unintentionally activated and recorded private conversations — some allegedly shared with third parties — for anyone who owned a Siri-enabled device between 2014 and 2024.",
    year: "2025",
    severity: "HIGH",
    sources: [
      { url: "https://www.courthousenews.com/judge-approves-95-million-apple-settlement-over-siri-privacy-case/", label: "Courthouse News Service" },
      { url: "https://www.cbsnews.com/news/apple-siri-settlement-95-million-lopez-how-to-file-claim/", label: "CBS News" },
      { url: "https://www.axios.com/2025/05/13/apple-lopez-voice-assistant-settlement-siri", label: "Axios" },
    ],
  },
  {
    entity: "apple",
    headline: "Apple dropped encrypted iCloud backups after the FBI privately objected",
    explanation:
      "Reuters reported Apple dropped a plan to offer end-to-end encrypted iCloud device backups around 2018, after the FBI's cyber crime division privately objected that it would hinder criminal investigations.",
    year: "2020",
    severity: "HIGH",
    sources: [
      { url: "https://www.investing.com/news/stock-market-news/exclusive-apple-dropped-plan-for-encrypting-backups-after-fbi-complained--sources-2063709", label: "Reuters" },
      { url: "https://appleinsider.com/articles/20/01/21/apple-dropped-plans-to-encrypt-icloud-after-the-fbi-complained", label: "AppleInsider" },
    ],
  },

  // ---- OpenAI ----
  {
    entity: "openai",
    headline: "ChatGPT trains on your conversations by default unless you opt out",
    explanation:
      "By default OpenAI may use content from consumer ChatGPT conversations (Free, Plus, Pro, Go) to train its models unless the user opts out; Business, Enterprise, and API accounts are excluded unless they opt in.",
    year: "Ongoing",
    severity: "HIGH",
    sources: [
      { url: "https://help.openai.com/en/articles/7039943-data-usage-for-consumer-services-faq", label: "OpenAI Help Center" },
      { url: "https://openai.com/policies/how-your-data-is-used-to-improve-model-performance/", label: "OpenAI" },
    ],
  },
  {
    entity: "openai",
    headline: "OpenAI actively blocks ChatGPT access from China, Russia, and Iran",
    explanation:
      "OpenAI restricts ChatGPT and API access to an allowlist of supported countries. Starting July 9, 2024 it began actively blocking API traffic from unsupported regions, notably mainland China, Russia, and Iran.",
    year: "2024",
    severity: "HIGH",
    sources: [
      { url: "https://developers.openai.com/api/docs/supported-countries", label: "OpenAI" },
      { url: "https://www.bankinfosecurity.com/openai-drops-chatgpt-access-for-users-in-china-russia-iran-a-25631", label: "BankInfoSecurity" },
    ],
  },
  {
    entity: "openai",
    headline: "OpenAI put the NSA's longest-serving director on its safety board",
    explanation:
      "In June 2024 OpenAI appointed retired General Paul Nakasone — NSA director and US Cyber Command commander from 2018 to 2024 — to its board, including its Safety and Security Committee.",
    year: "2024",
    severity: "HIGH",
    sources: [
      { url: "https://openai.com/index/openai-appoints-retired-us-army-general/", label: "OpenAI" },
      { url: "https://www.washingtonpost.com/technology/2024/06/13/openai-board-paul-nakasone-nsa/", label: "The Washington Post" },
      { url: "https://www.cio.com/article/2152275/whats-behind-openais-appointment-of-an-ex-nsa-director-to-its-board.html", label: "CIO.com" },
    ],
    image: "chatgpt-nsa-director.jpg",
  },
  {
    entity: "openai",
    headline: "OpenAI quietly dropped its ban on military and weapons use",
    explanation:
      "On January 10, 2024 OpenAI quietly removed its blanket ban on 'military and warfare' use and weapons development from its usage policy, replacing it with a narrower prohibition on harming people.",
    year: "2024",
    severity: "HIGH",
    sources: [
      { url: "https://techcrunch.com/2024/01/12/openai-changes-policy-to-allow-military-applications/", label: "TechCrunch" },
      { url: "https://theintercept.com/2024/01/12/open-ai-military-ban-chatgpt/", label: "The Intercept" },
    ],
  },

  // ---- Zoom ----
  {
    entity: "zoom",
    headline: "A US-Canada Zoom call's encryption key routed through a Beijing server",
    explanation:
      "Citizen Lab found that in a test call between US and Canadian participants, Zoom's meeting encryption key was distributed from a server apparently located in Beijing. Zoom blamed a mistaken backup-server whitelist and reversed the change.",
    year: "2020",
    severity: "CRITICAL",
    sources: [
      { url: "https://citizenlab.ca/2020/04/move-fast-roll-your-own-crypto-a-quick-look-at-the-confidentiality-of-zoom-meetings/", label: "Citizen Lab" },
      { url: "https://techcrunch.com/2020/04/03/zoom-calls-routed-china/", label: "TechCrunch" },
    ],
  },
  {
    entity: "zoom",
    headline: "Zoom claimed 'end-to-end encryption' it wasn't actually providing",
    explanation:
      "The FTC alleged Zoom misled users since at least 2016 by claiming 'end-to-end, 256-bit encryption' it didn't actually provide, retaining access to meeting content. Zoom settled in November 2020 with no fine, agreeing to security audits.",
    year: "2020",
    severity: "HIGH",
    sources: [
      { url: "https://www.goodwinlaw.com/en/insights/publications/2020/11/11_18-ftc-and-zoom-reach-settlement-over-alleged", label: "Goodwin Procter" },
      { url: "https://fortune.com/2020/11/09/zoom-ftc-settlement-fine-security-privacy/", label: "Fortune" },
      { url: "https://techcrunch.com/2020/11/09/zoom-ftc-deceptive-security-claims/", label: "TechCrunch" },
    ],
  },

  // ---- Oracle ----
  {
    entity: "oracle",
    headline: "Oracle is named after the CIA project that was its first customer",
    explanation:
      "Oracle Corporation, founded in 1977 by Larry Ellison, Bob Miner, and Ed Oates, took its name from the codename of a CIA database project — the CIA was Oracle's first customer.",
    year: "1977",
    severity: "HIGH",
    sources: [
      { url: "https://gizmodo.com/larry-ellisons-oracle-started-as-a-cia-project-1636592238", label: "Gizmodo" },
      { url: "https://en.wikipedia.org/wiki/Oracle_Corporation", label: "Wikipedia" },
    ],
  },
  {
    entity: "oracle",
    headline: "A former CIA director joined Oracle's board in 2015",
    explanation:
      "Leon Panetta, CIA Director from 2009-2011 and Defense Secretary from 2011-2013, joined Oracle's board of directors effective January 2015.",
    year: "2015",
    severity: "HIGH",
    sources: [
      { url: "https://investor.oracle.com/investor-news/news-details/2015/Oracle-Names-Leon-Panetta-to-the-Board-of-Directors/default.aspx", label: "Oracle Investor Relations" },
      { url: "https://www.itnews.com.au/news/former-cia-boss-joins-oracle-399524", label: "iTnews" },
    ],
  },

  // ---- Adobe ----
  {
    entity: "adobe",
    headline: "Adobe cut off every Venezuelan customer's Creative Cloud access overnight",
    explanation:
      "In October 2019 Adobe announced it would deactivate all Venezuelan customer accounts, cutting off Creative Cloud and Document Cloud, to comply with a US executive order — then secured a license days later to keep serving Venezuela uninterrupted.",
    year: "2019",
    severity: "CRITICAL",
    sources: [
      { url: "https://blog.adobe.com/en/publish/2019/10/28/adobe-continues-digital-media-access-in-venezuela", label: "Adobe" },
      { url: "https://www.engadget.com/2019-10-08-adobe-venezuela-executive-order.html", label: "Engadget" },
      { url: "https://www.theregister.com/2019/10/10/adobe_venezuela_sanctions/", label: "The Register" },
    ],
  },

  // ---- Slack ----
  {
    entity: "slack",
    headline: "Slack deactivated accounts just for logging in from sanctioned countries",
    explanation:
      "In December 2018 Slack deactivated accounts it linked by IP address to sanctioned countries, catching people who'd merely traveled through or logged in from them. After backlash, Slack restored most accounts and switched to blocking access only during active connections.",
    year: "2018",
    severity: "HIGH",
    sources: [
      { url: "https://techcrunch.com/2018/12/22/slack-says-it-will-comply-with-sanctions/", label: "TechCrunch" },
      { url: "https://www.engadget.com/2018-12-22-iran-sanctions-slack.html", label: "Engadget" },
      { url: "https://9to5mac.com/2018/12/22/slack-iran-deactivating-accounts/", label: "9to5Mac" },
    ],
  },

  // ---- NSA ----
  {
    entity: "nsa",
    headline: "Almost everything we know about NSA mass surveillance came from one leaker",
    explanation:
      "Most public knowledge of NSA mass-surveillance capabilities comes from documents Edward Snowden leaked starting in June 2013, when The Guardian and Washington Post published revelations including bulk telephone metadata collection and PRISM.",
    year: "2013",
    severity: "UNKNOWN",
    sources: [
      { url: "https://www.pbs.org/wgbh/frontline/article/how-the-nsa-spying-programs-have-changed-since-snowden/", label: "PBS Frontline" },
      { url: "https://en.wikipedia.org/wiki/Snowden_disclosures", label: "Wikipedia" },
    ],
  },
  {
    entity: "nsa",
    headline: "The NSA impersonated Facebook servers to secretly inject malware into targets",
    explanation:
      "According to a 2014 Intercept report, the NSA developed 'QUANTUM' techniques — including impersonating Facebook servers — to hijack web requests and inject malware, using an automation system built to scale to millions of implants.",
    year: "2014",
    severity: "HIGH",
    sources: [
      { url: "https://theintercept.com/2014/03/12/nsa-plans-infect-millions-computers-malware/", label: "The Intercept" },
    ],
  },
  {
    entity: "nsa",
    headline: "One NSA tool let analysts search your emails and browsing with a simple query",
    explanation:
      "XKeyscore is an NSA system, disclosed by Snowden in 2013, that let analysts search vast troves of intercepted emails, chats, and browsing history via simple queries, with oversight applied only after the fact.",
    year: "2013",
    severity: "CRITICAL",
    sources: [
      { url: "https://theintercept.com/2015/07/01/nsas-google-worlds-private-communications/", label: "The Intercept" },
      { url: "https://en.wikipedia.org/wiki/XKeyscore", label: "Wikipedia" },
    ],
  },
  {
    entity: "nsa",
    headline: "The NSA pulled user data directly from Google, Apple, and Facebook",
    explanation:
      "The PRISM program let the NSA collect user communications data directly from major tech companies — Microsoft, Google, Facebook, Apple, Yahoo, and others — under Section 702 of FISA, revealed by Snowden documents in June 2013.",
    year: "2013",
    severity: "CRITICAL",
    sources: [
      { url: "https://en.wikipedia.org/wiki/PRISM", label: "Wikipedia" },
    ],
    image: "nsa-prism-slide.png",
  },
  {
    entity: "nsa",
    headline: "A secret NSA program worked to break the encryption protecting your bank and email",
    explanation:
      "Leaked documents revealed BULLRUN, a classified NSA program using supercomputers, court orders, hacking, and covert industry collaboration to defeat encryption protecting commerce, banking, and medical records. GCHQ's parallel program was cracking VPN traffic for dozens of targets by 2010.",
    year: "2013",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.theguardian.com/world/2013/sep/05/nsa-gchq-encryption-codes-security", label: "The Guardian" },
      { url: "https://www.propublica.org/article/the-nsas-secret-campaign-to-crack-undermine-internet-encryption", label: "ProPublica" },
      { url: "https://en.wikipedia.org/wiki/Bullrun_(decryption_program)", label: "Wikipedia" },
    ],
  },
  {
    entity: "nsa",
    headline: "The NSA spent $800 million pushing tech companies to build exploitable encryption",
    explanation:
      "A leaked 2013 budget document revealed the NSA's SIGINT Enabling Project, funded over $800 million since 2011, which worked with chipmakers to insert backdoors into encryption chips and influence commercial cryptography standards.",
    year: "2013",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.nytimes.com/interactive/2013/09/05/us/documents-reveal-nsa-campaign-against-encryption.html", label: "New York Times" },
      { url: "https://www.theguardian.com/world/2013/sep/05/nsa-gchq-encryption-codes-security", label: "The Guardian" },
      { url: "https://www.eff.org/node/77502", label: "EFF" },
    ],
  },
  {
    entity: "nsa",
    headline: "The NSA quietly engineered a flawed encryption standard it could crack",
    explanation:
      "The NSA pushed the Dual_EC_DRBG random-number generator through NIST standardization, privately working to become its 'sole editor.' Researchers showed in 2007 its constants could let a key-holder predict all future outputs; NIST withdrew it in 2014.",
    year: "2006–14",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.nytimes.com/interactive/2013/09/05/us/documents-reveal-nsa-campaign-against-encryption.html", label: "New York Times" },
      { url: "https://en.wikipedia.org/wiki/Dual_EC_DRBG", label: "Wikipedia" },
      { url: "https://arstechnica.com/information-technology/2013/09/stop-using-nsa-influenced-code-in-our-product-rsa-tells-customers/", label: "Ars Technica" },
    ],
  },
  {
    entity: "nsa",
    headline: "A leaked NSA catalog listed a 100%-success implant for hacking iPhones",
    explanation:
      "A leaked 2013 NSA catalog revealed DROPOUTJEEP, a software implant for the iPhone offering SMS, contact, voicemail, location, and camera/microphone access, with a claimed 100% installation success rate. Apple denied any collaboration.",
    year: "2013",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.eff.org/files/2014/01/06/20131230-appelbaum-nsa_ant_catalog.pdf", label: "EFF (NSA ANT Catalog PDF)" },
      { url: "https://en.wikipedia.org/wiki/ANT_catalog", label: "Wikipedia" },
    ],
    image: "nsa-ant-dropoutjeep.jpg",
  },
  {
    entity: "nsa",
    headline: "The NSA sold $20,000 USB plugs rigged to secretly hack air-gapped computers",
    explanation:
      "The leaked NSA ANT catalog detailed COTTONMOUTH, covert USB hardware implants — priced around $20,000-25,000 per unit — that hide a radio transceiver to bridge air-gapped networks and enable undetected data exfiltration.",
    year: "2013",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.eff.org/files/2014/01/06/20131230-appelbaum-nsa_ant_catalog.pdf", label: "EFF (NSA ANT Catalog PDF)" },
      { url: "https://gizmodo.com/a-peek-inside-the-nsas-spy-gear-catalog-1491827763", label: "Gizmodo" },
    ],
    image: "nsa-ant-cottonmouth-i.jpg",
  },
  {
    entity: "nsa",
    headline: "The NSA intercepted shipped routers to secretly implant hacking beacons",
    explanation:
      "A leaked 2010 NSA memo described how its Tailored Access Operations unit intercepted US-exported routers and servers in transit, implanted beacons in secret workshops, then resealed and forwarded the repackaged hardware to its destination.",
    year: "2010",
    severity: "CRITICAL",
    sources: [
      { url: "https://techcrunch.com/2014/05/13/nsa-reportedly-intercepts-and-alters-routers-and-servers-exported-from-u-s-to-facilitate-surveillance/", label: "TechCrunch" },
      { url: "https://www.theregister.com/2014/05/13/greenwald_alleges_nsa_tampers_with_routers_to_plant_backdoors", label: "The Register" },
    ],
  },
  {
    entity: "nsa",
    headline: "NSA servers raced real websites to secretly redirect users into traps",
    explanation:
      "Snowden documents revealed the NSA's QUANTUM program: servers positioned on the internet backbone raced legitimate responses to redirect targets to FOXACID, malware that deanonymized Tor users; GCHQ used the same technique against Belgacom engineers and OPEC's Vienna headquarters.",
    year: "2013",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.schneier.com/blog/archives/2013/10/how_the_nsa_att.html", label: "Schneier on Security (orig. The Guardian, Oct 4, 2013)" },
      { url: "https://www.schneier.com/blog/archives/2013/11/another_quantum.html", label: "Schneier on Security — Another QUANTUMINSERT Attack Example (Nov 13, 2013, citing Der Spiegel)" },
    ],
  },
  {
    entity: "nsa",
    headline: "The NSA tapped Google and Yahoo's private cables between their own data centers",
    explanation:
      "The NSA and GCHQ jointly tapped fiber-optic links between Google and Yahoo data centers worldwide under project MUSCULAR, exploiting the unencrypted point where traffic left public networks. One 30-day count showed over 181 million new records sent to NSA headquarters.",
    year: "2013",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.washingtonpost.com/world/national-security/nsa-infiltrates-links-to-yahoo-google-data-centers-worldwide-snowden-documents-say/2013/10/30/e51d661e-4166-11e3-8b74-d89d714ca4dd_story.html", label: "The Washington Post" },
      { url: "https://www.npr.org/sections/thetwo-way/2013/10/30/241855353/report-nsa-has-broken-into-google-and-yahoo-data-centers", label: "NPR" },
    ],
    image: "nsa-muscular-google-cloud-slide.jpg",
  },
  {
    entity: "nsa",
    headline: "The NSA secretly tracked 97 billion pieces of intelligence in one month",
    explanation:
      "Boundless Informant is an NSA tool that generated a color-coded heat map of metadata collection by country. Leaked records showed almost 3 billion pieces of US intelligence and 97 billion worldwide collected in one 30-day period.",
    year: "2013",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.theguardian.com/world/2013/jun/08/nsa-boundless-informant-global-datamining", label: "The Guardian" },
      { url: "https://www.engadget.com/2013-06-08-the-nsas-boundless-informant.html", label: "Engadget" },
    ],
    image: "boundless-informant-heat-map.svg",
  },
  {
    entity: "nsa",
    headline: "AT&T let the NSA install surveillance gear in its own internet hubs for decades",
    explanation:
      "FAIRVIEW is the NSA's surveillance partnership with AT&T dating to 1985. Documents show AT&T installed NSA equipment in at least 17 US internet hubs, forwarding over a million emails daily and roughly 400 billion metadata records monthly by 2003.",
    year: "1985–2013 (disclosed 2015)",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.propublica.org/article/nsa-spying-relies-on-atts-extreme-willingness-to-help", label: "ProPublica" },
      { url: "https://www.propublica.org/article/a-trail-of-evidence-leading-to-atts-partnership-with-the-nsa", label: "ProPublica" },
    ],
  },
  {
    entity: "nsa",
    headline: "Verizon's fiber cables became the NSA's second-biggest surveillance pipeline",
    explanation:
      "STORMBREW is an NSA program tapping fiber-optic infrastructure via a corporate partner identified as Verizon, confirmed by a 2015 ProPublica/New York Times investigation. It was the NSA's second-largest corporate collection program, after FAIRVIEW.",
    year: "2013",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.propublica.org/article/a-trail-of-evidence-leading-to-atts-partnership-with-the-nsa", label: "ProPublica" },
      { url: "https://en.wikipedia.org/wiki/STORMBREW", label: "Wikipedia — STORMBREW" },
    ],
  },
  {
    entity: "nsa",
    headline: "The NSA vacuumed up 194 million text messages a day, worldwide",
    explanation:
      "DISHFIRE is an NSA/GCHQ database that collected roughly 194 million SMS texts per day worldwide by 2011, regardless of whether senders were targets, mining contact lists, geolocation, financial transactions, and border-crossing records.",
    year: "2014",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.theguardian.com/world/2014/jan/16/nsa-collects-millions-text-messages-daily-untargeted-global-sweep", label: "The Guardian" },
      { url: "https://www.npr.org/sections/thetwo-way/2014/01/16/263130142/nsa-reportedly-collected-millions-of-phone-texts-every-day", label: "NPR" },
    ],
  },
  {
    entity: "nsa",
    headline: "The NSA collects nearly 5 billion cellphone location records every day",
    explanation:
      "CO-TRAVELER is an NSA program gathering nearly 5 billion cellphone location records daily from global mobile network cables, used to map which phones travel together and infer hidden associations — incidentally sweeping up Americans abroad.",
    year: "2013",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.washingtonpost.com/world/national-security/nsa-tracking-cellphone-locations-worldwide-snowden-documents-show/2013/12/04/5492873a-5cf2-11e3-bc56-c6ca94801fac_story.html", label: "The Washington Post" },
      { url: "https://www.eff.org/deeplinks/2013/12/meet-co-traveler-nsas-cell-phone-location-tracking-program", label: "EFF" },
    ],
  },
  {
    entity: "nsa",
    headline: "Leaked NSA hacking tools escaped into the wild and were never recovered",
    explanation:
      "Starting in 2016, the anonymous Shadow Brokers leaked NSA offensive hacking tools; an April 2017 dump included the EternalBlue exploit. Microsoft had quietly patched the underlying flaw a month earlier, suggesting an advance NSA tip-off. No one has ever been charged.",
    year: "2016–17",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.rapid7.com/blog/post/2017/04/18/the-shadow-brokers-leaked-exploits-faq/", label: "Rapid7" },
      { url: "https://en.wikipedia.org/wiki/The_Shadow_Brokers", label: "Wikipedia" },
    ],
  },
  {
    entity: "nsa",
    headline: "A leaked NSA exploit powered ransomware that crippled hospitals worldwide",
    explanation:
      "The leaked NSA EternalBlue exploit powered WannaCry ransomware, which hit 150+ countries in May 2017 and crippled parts of the UK's NHS, and NotPetya, which caused an estimated $10 billion in global damage weeks later.",
    year: "2017",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.cisa.gov/news-events/alerts/2017/05/12/indicators-associated-wannacry-ransomware", label: "CISA" },
      { url: "https://www.justice.gov/archives/opa/pr/six-russian-gru-officers-charged-connection-worldwide-deployment-destructive-malware-and", label: "DOJ" },
      { url: "https://www.fbi.gov/wanted/cyber/gru-hackers-destructive-malware-and-international-cyber-attacks", label: "FBI" },
    ],
  },
  {
    entity: "nsa",
    headline: "NSA-built malware hid inside hard-drive firmware, surviving full disk wipes",
    explanation:
      "Kaspersky detailed the 'Equation Group,' active since at least 2001, whose malware reprogrammed hard-drive firmware from major manufacturers, surviving disk wipes and OS reinstalls. Reuters, citing former NSA officials, reported the tools were built by the NSA.",
    year: "2015",
    severity: "CRITICAL",
    sources: [
      { url: "https://securelist.com/equation-the-death-star-of-malware-galaxy/68750/", label: "Kaspersky Securelist" },
      { url: "https://www.eff.org/deeplinks/2015/02/russian-researchers-uncover-sophisticated-malware-equation-group", label: "EFF" },
    ],
  },
  {
    entity: "nsa",
    headline: "Spies stole millions of SIM card encryption keys from one manufacturer",
    explanation:
      "A leaked GCHQ document describes how NSA and GCHQ's Mobile Handset Exploitation Team penetrated SIM maker Gemalto, harvesting millions of encryption keys within a documented three-month window in 2010, including 300,000 keys from one Somali carrier.",
    year: "2010",
    severity: "CRITICAL",
    sources: [
      { url: "https://theintercept.com/2015/02/19/great-sim-heist/", label: "The Intercept" },
      { url: "https://www.thalesgroup.com/en/markets/digital-identity-and-security/press-release/gemalto-presents-the-findings-of-its-investigations-into-the-alleged-hacking-of-sim-card-encryption-keys", label: "Thales/Gemalto press release" },
    ],
  },
  {
    entity: "nsa",
    headline: "The NSA tapped Angela Merkel's personal cellphone for over a decade",
    explanation:
      "Der Spiegel reported the NSA monitored German Chancellor Angela Merkel's personal mobile phone since 2002. A separate leaked 2006 memo showed 35 world leaders' numbers were tasked for surveillance. Merkel called Obama to demand an explanation.",
    year: "2002–13",
    severity: "HIGH",
    sources: [
      { url: "https://www.france24.com/en/20131027-germany-report-usa-bugged-merkel-phone", label: "France 24" },
      { url: "https://www.benton.org/headlines/nsa-monitored-calls-35-world-leaders-after-us-official-handed-over-contacts", label: "Benton Institute (citing The Guardian)" },
      { url: "https://www.nbcnews.com/storyline/nsa-snooping/germany-drops-nsa-merkel-cellphone-spying-probe-lacking-evidence-n374206", label: "NBC News" },
    ],
    image: "angela-merkel-portrait-2011.jpg",
  },
  {
    entity: "nsa",
    headline: "The NSA built a $1.5 billion facility to store intercepted data at massive scale",
    explanation:
      "The NSA's Utah Data Center, completed in 2014 at a cost of about $1.5 billion, is a roughly 1-million-square-foot complex reportedly designed to intercept, store, and analyze communications at massive scale as part of the NSA's post-9/11 buildout.",
    year: "2011–2014",
    severity: "CRITICAL",
    sources: [
      { url: "https://en.wikipedia.org/wiki/Utah_Data_Center", label: "Wikipedia" },
      { url: "https://clui.org/ludb/site/nsa-utah-data-center", label: "Center for Land Use Interpretation" },
      { url: "https://www.eff.org/deeplinks/2014/07/releasing-public-domain-image-nsas-utah-data-center", label: "EFF" },
    ],
    image: "nsa-utah-data-center-aerial.jpg",
  },
  {
    entity: "nsa",
    headline: "GCHQ tapped transatlantic cables and fed the raw data to the NSA",
    explanation:
      "GCHQ's TEMPORA program, operational by 2011, tapped transatlantic fiber-optic cables landing in Britain to buffer internet content for up to three days and metadata for thirty, sharing the intercepted data with the NSA.",
    year: "2013",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.npr.org/sections/thetwo-way/2013/06/21/194267403/report-uk-spy-agency-taps-trans-atlantic-fiber-optic-cables", label: "NPR" },
      { url: "https://en.wikipedia.org/wiki/Tempora", label: "Wikipedia (secondary corroboration)" },
      { url: "https://www.eff.org/document/20140618-der-spiegel-gchq-report-technical-abilities-tempora", label: "EFF (leaked GCHQ document)" },
    ],
  },
  {
    entity: "nsa",
    headline: "GCHQ grabbed webcam stills from 1.8 million Yahoo accounts, nudity included",
    explanation:
      "GCHQ's OPTIC NERVE program, built with NSA support, bulk-collected still webcam images from Yahoo video chats every five minutes regardless of surveillance status, capturing images from 1.8 million accounts in one six-month period; 3-11% contained nudity.",
    year: "2014",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.npr.org/2014/02/28/283999713/joint-surveillance-program-stores-millions-of-yahoo-webcam-images", label: "NPR" },
      { url: "https://en.wikipedia.org/wiki/Optic_Nerve_(GCHQ)", label: "Wikipedia (secondary corroboration)" },
      { url: "https://thehackernews.com/2014/02/optic-nerve-dirty-nsa-hacked-into.html", label: "The Hacker News" },
    ],
  },
  {
    entity: "nsa",
    headline: "Rogue spyware wiretapped Greece's prime minister — then the engineer who found it died",
    explanation:
      "Between 2004 and 2005 a rootkit wiretapped over 100 phones on Vodafone Greece's network, including the Prime Minister and cabinet, before Ericsson discovered and removed it. The network manager who found it was found dead days later; attribution remains contested.",
    year: "2004–05",
    severity: "UNKNOWN",
    sources: [
      { url: "https://spectrum.ieee.org/the-athens-affair", label: "IEEE Spectrum" },
      { url: "https://theintercept.com/2015/09/28/death-athens-rogue-nsa-operation/", label: "The Intercept" },
      { url: "https://en.wikipedia.org/wiki/Greek_wiretapping_case_2004%E2%80%9305", label: "Wikipedia (secondary corroboration)" },
    ],
  },
  {
    entity: "nsa",
    headline: "A former NSA chief said some security flaws are fine to leave unpatched",
    explanation:
      "A former NSA/CIA director publicly explained the agency's 'NOBUS' (nobody-but-us) reasoning: a vulnerability too hard for anyone but the US to exploit isn't ethically or legally required to be patched, confirming the NSA weighs offensive value against disclosure.",
    year: "2013",
    severity: "HIGH",
    sources: [
      { url: "https://www.washingtonpost.com/news/the-switch/wp/2013/10/04/why-everyone-is-left-less-secure-when-the-nsa-doesnt-help-fix-security-flaws/", label: "The Washington Post" },
      { url: "https://en.wikipedia.org/wiki/NOBUS", label: "Wikipedia" },
    ],
    image: "michael-hayden-portrait.jpg",
  },
  {
    entity: "nsa",
    headline: "The NSA was accused of exploiting Heartbleed for years instead of reporting it",
    explanation:
      "Bloomberg reported the NSA knew of the Heartbleed OpenSSL flaw for at least two years and regularly exploited it for intelligence rather than disclosing it; the NSA and ODNI issued same-day denials.",
    year: "2014",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.bloomberg.com/news/articles/2014-04-11/nsa-said-to-have-used-heartbleed-bug-exposing-consumers", label: "Bloomberg" },
      { url: "https://www.npr.org/sections/thetwo-way/2014/04/11/301967026/nsa-denies-it-knew-about-heartbleed-bug-before-it-was-made-public", label: "NPR" },
    ],
  },

  // ---- US Government ----
  {
    entity: "usgov",
    headline: "US law can force Google or Facebook to hand over your data stored anywhere",
    explanation:
      "The 2018 US CLOUD Act lets US law enforcement compel American providers like Google or Facebook to produce a user's data via warrant or subpoena, even when it's stored on servers abroad, bypassing the foreign country's own privacy laws.",
    year: "2018",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.eff.org/deeplinks/2018/02/cloud-act-dangerous-expansion-police-snooping-cross-border-data", label: "Electronic Frontier Foundation" },
    ],
  },
  {
    entity: "usgov",
    headline: "A foreign-surveillance law also enables warrantless FBI searches of Americans' messages",
    explanation:
      "Section 702 of FISA, enacted in 2008, nominally authorizes surveillance of non-US persons abroad using US communications providers, but in practice its implementation also sweeps in and enables warrantless FBI searches of Americans' communications.",
    year: "2008",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.eff.org/702-spying", label: "Electronic Frontier Foundation" },
    ],
  },
  {
    entity: "usgov",
    headline: "The government once tried to put a backdoor chip in every phone",
    explanation:
      "In 1993 the Clinton White House announced the Clipper Chip, a hardware encryption device whose cryptographic keys were split and escrowed with the government, letting law enforcement decrypt calls under legal authorization. Opposition killed it by 1996.",
    year: "1993",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.eff.org/deeplinks/2015/04/clipper-chips-birthday-looking-back-22-years-key-escrow-failures", label: "EFF" },
      { url: "https://en.wikipedia.org/wiki/Clipper_chip", label: "Wikipedia" },
      { url: "https://w2.eff.org/Privacy/Key_escrow/Clipper/", label: "EFF Archive" },
    ],
    image: "myk-78-clipper-chip-markings.jpg",
  },
  {
    entity: "usgov",
    headline: "A researcher broke the government's phone-encryption backdoor within a year",
    explanation:
      "In 1994 AT&T Bell Labs cryptographer Matt Blaze showed Clipper's Law Enforcement Access Field relied on a checksum too short to prevent tampering, letting an attacker keep Clipper's encryption while defeating the government's wiretap-escrow mechanism.",
    year: "1994",
    severity: "HIGH",
    sources: [
      { url: "https://www.nist.gov/news-events/news/1994/06/statement-response-blaze-key-escrow-paper", label: "NIST" },
      { url: "https://dl.acm.org/doi/10.1145/191177.191193", label: "ACM Digital Library" },
      { url: "https://en.wikipedia.org/wiki/Clipper_chip", label: "Wikipedia" },
    ],
  },
  {
    entity: "usgov",
    headline: "Bush secretly authorized warrantless spying on Americans' calls and email after 9/11",
    explanation:
      "President Bush authorized STELLARWIND in October 2001, bypassing FISA warrant requirements to collect bulk telephone and internet metadata plus some content on Americans via direct access to telecom carriers. First revealed by the New York Times in 2005.",
    year: "2001–2007",
    severity: "CRITICAL",
    sources: [
      { url: "https://oig.justice.gov/reports/report-presidents-surveillance-program-unclassified-prepared-offices-inspectors-general", label: "DOJ Office of Inspector General" },
      { url: "https://www.aclu.org/files/natsec/nsa/20130816/NSA%20IG%20Report.pdf", label: "ACLU (declassified NSA IG Report)" },
      { url: "https://www.eff.org/files/filenode/foia_C0705278/022908_ex_a-d_0.pdf", label: "EFF (NYT: \"Bush Lets U.S. Spy on Callers Without Courts\")" },
    ],
  },
  {
    entity: "usgov",
    headline: "A 1994 law forced telecoms to build wiretap access into every network",
    explanation:
      "President Clinton signed CALEA in 1994, requiring telecom carriers to build law-enforcement intercept capability directly into their networks. The FCC extended the mandate to broadband and VoIP in 2005.",
    year: "1994",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.fcc.gov/calea", label: "FCC" },
      { url: "https://www.congress.gov/bill/103rd-congress/house-bill/4922", label: "Congress.gov" },
      { url: "https://www.eff.org/issues/calea", label: "EFF" },
    ],
  },
  {
    entity: "usgov",
    headline: "Chinese hackers broke into the wiretap systems built for US law enforcement",
    explanation:
      "Chinese state-linked hackers breached the CALEA-mandated lawful-intercept systems at AT&T, Verizon, and other carriers, disclosed in October 2024, obtaining near-complete lists of phone numbers under active US wiretap orders and metadata for over a million users.",
    year: "2024",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.cisa.gov/news-events/alerts/2024/12/03/cisa-and-partners-release-joint-guidance-prc-affiliated-threat-actor-compromising-networks-global", label: "CISA" },
      { url: "https://www.eff.org/deeplinks/2024/10/salt-typhoon-hack-shows-theres-no-security-backdoor-thats-only-good-guys", label: "EFF" },
      { url: "https://www.axios.com/2024/10/15/salt-typhoon-hack-china-verizon-att", label: "Axios" },
    ],
  },
  {
    entity: "usgov",
    headline: "The FBI collected everyone's phone call records for nearly a decade",
    explanation:
      "A secret 2006 FISA Court order re-grounded bulk telephone-metadata collection in Section 215 of the PATRIOT Act, compelling carriers to hand the NSA daily call records for effectively all Americans, retained up to five years, until reforms ended it in 2015.",
    year: "2006",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.csis.org/analysis/fact-sheet-section-215-usa-patriot-act", label: "CSIS" },
      { url: "https://www.lawfaremedia.org/article/nsa-ends-bulk-collection-telephony-metadata-under-section-215", label: "Lawfare" },
    ],
  },
  {
    entity: "usgov",
    headline: "A student had to sue the government just to publish his own encryption code",
    explanation:
      "A PhD student sued the State Department in 1995 after export-control rules required a government license just to publish his encryption source code. A federal judge ruled twice that source code is First Amendment-protected speech.",
    year: "1995–99",
    severity: "HIGH",
    sources: [
      { url: "https://www.eff.org/cases/bernstein-v-us-dept-justice", label: "EFF" },
      { url: "https://law.justia.com/cases/federal/district-courts/FSupp/945/1279/1457799/", label: "Justia (Bernstein v. Dept. of State, 945 F. Supp. 1279)" },
    ],
  },
  {
    entity: "usgov",
    headline: "A grand jury investigated the creator of PGP for publishing free encryption",
    explanation:
      "After Phil Zimmermann released PGP encryption freely in 1991, US Customs opened a criminal investigation for suspected arms-export violations, and a federal grand jury weighed charges risking years in prison. The government dropped the case without indictment in 1996.",
    year: "1993–96",
    severity: "HIGH",
    sources: [
      { url: "https://en.wikipedia.org/wiki/Phil_Zimmermann", label: "Wikipedia" },
      { url: "https://dubois.com/pgp-case/", label: "duboislaw (PGP Case case file)" },
    ],
  },
  {
    entity: "usgov",
    headline: "A Senate bill would pressure platforms to abandon end-to-end encryption",
    explanation:
      "The EARN IT Act would strip legal immunity from platforms that don't follow a government-backed 'best practices' commission on child-exploitation content. EFF warned it pressures providers to abandon end-to-end encryption or build client-side scanning; it has stalled without a floor vote.",
    year: "2020–22",
    severity: "HIGH",
    sources: [
      { url: "https://www.eff.org/deeplinks/2020/07/new-earn-it-bill-still-threatens-encryption-and-free-speech", label: "EFF" },
      { url: "https://www.eff.org/deeplinks/2022/02/key-senators-have-voted-anti-encryption-earn-it-act", label: "EFF" },
    ],
  },
  {
    entity: "usgov",
    headline: "The government finally pulled its own NSA-tainted encryption standard in 2014",
    explanation:
      "In April 2014 NIST formally removed Dual_EC_DRBG from its cryptography guidance, ending its seven-year run as a federally approved random-number generator, after a 2013 New York Times report alleged the NSA had engineered a predictable weakness into it.",
    year: "2014",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.nist.gov/news-events/news/2014/04/nist-removes-cryptography-algorithm-random-number-generator-recommendations", label: "NIST" },
      { url: "https://en.wikipedia.org/wiki/NIST_SP_800-90A", label: "Wikipedia" },
    ],
  },
  {
    entity: "usgov",
    headline: "A federal panel warned NIST it must be able to reject NSA advice",
    explanation:
      "After allegations the NSA deliberately weakened a NIST algorithm, an independent panel including Vint Cerf and Ron Rivest reviewed NIST's standards process in 2014, concluding NIST may consult the NSA but \"must be in a position to assess it and reject it when warranted.\"",
    year: "2014",
    severity: "HIGH",
    sources: [
      { url: "https://www.nist.gov/news-events/news/2014/07/nist-advisory-group-releases-report-cryptography-expertise-and-standards", label: "NIST" },
      { url: "https://www.lawfaremedia.org/article/nsas-subversion-nists-algorithm", label: "Lawfare" },
    ],
  },
  {
    entity: "usgov",
    headline: "The White House admitted it sometimes keeps hacking flaws secret on purpose",
    explanation:
      "In April 2014 the White House published the government's first public acknowledgment of how it decides whether to disclose zero-day vulnerabilities — a process \"biased toward responsibly disclosing\" flaws but with \"no hard and fast rules.\"",
    year: "2014",
    severity: "HIGH",
    sources: [
      { url: "https://obamawhitehouse.archives.gov/blog/2014/04/28/heartbleed-understanding-when-we-disclose-cyber-vulnerabilities", label: "The White House (Obama Archives)" },
      { url: "https://nsarchive.gwu.edu/document/17627-white-house-heartbleed-understanding-when-we", label: "National Security Archive" },
    ],
  },

  // ---- CIA ----
  {
    entity: "cia",
    headline: "The CIA secretly owned an encryption company selling to 100+ governments",
    explanation:
      "From 1970 until 2018, the CIA and West Germany's BND secretly owned Swiss firm Crypto AG and manipulated its encryption devices, letting them read the classified communications of roughly 100 of the 130 governments that bought its equipment.",
    year: "2020",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.washingtonpost.com/graphics/2020/world/national-security/cia-crypto-encryption-machines-espionage/", label: "Washington Post" },
      { url: "https://www.npr.org/2020/03/05/812499752/uncovering-the-cias-audacious-operation-that-gave-them-access-to-state-secrets", label: "NPR" },
      { url: "https://en.wikipedia.org/wiki/Operation_Rubicon", label: "Wikipedia" },
    ],
  },
  {
    entity: "cia",
    headline: "WikiLeaks exposed the CIA's entire hacking toolkit for phones and computers",
    explanation:
      "Starting March 2017, WikiLeaks published 'Vault 7,' the largest-ever publication of confidential CIA documents — thousands of files describing the agency's hacking tools, malware, and zero-day exploits for phones and computers.",
    year: "2017",
    severity: "CRITICAL",
    sources: [
      { url: "https://wikileaks.org/ciav7p1/", label: "WikiLeaks" },
      { url: "https://en.wikipedia.org/wiki/Vault_7", label: "Wikipedia" },
    ],
  },
  {
    entity: "cia",
    headline: "The CIA's venture arm funded the tech that became Google Earth",
    explanation:
      "In 2003 the CIA's venture arm, In-Q-Tel, invested in Keyhole Corp., maker of the 3D earth-visualization software EarthViewer. Google acquired Keyhole in 2004 and relaunched it in 2005 as Google Earth.",
    year: "2003",
    severity: "HIGH",
    sources: [
      { url: "https://www.iqt.org/library/in-q-tel-announces-strategic-investment-in-keyhole", label: "In" },
      { url: "https://en.wikipedia.org/wiki/In-Q-Tel", label: "Wikipedia" },
      { url: "https://fortune.com/2025/07/29/in-q-tel-cia-venture-capital-palantir-anduril/", label: "Fortune" },
    ],
  },
  {
    entity: "cia",
    headline: "A leaked CIA tool disguised its hacks to look like other countries did it",
    explanation:
      "WikiLeaks released 'Marble,' a CIA anti-forensic tool that hides text fragments in malware to hamper attribution, including test strings in Chinese, Russian, Korean, Arabic, and Farsi that could misdirect investigators toward other nations.",
    year: "2017",
    severity: "CRITICAL",
    sources: [
      { url: "https://wikileaks.org/vault7/", label: "WikiLeaks" },
      { url: "https://securityaffairs.com/57586/intelligence/vault7-marble-framework.html", label: "Security Affairs" },
    ],
  },
  {
    entity: "cia",
    headline: "The CIA turned smart TVs into hidden microphones that looked switched off",
    explanation:
      "WikiLeaks revealed 'Weeping Angel,' a CIA tool co-developed with UK intelligence that targeted Samsung smart TVs. Installed via USB, it placed the TV in 'Fake-Off' mode — screen dark, microphone still recording room audio and sending it to the CIA.",
    year: "2017",
    severity: "CRITICAL",
    sources: [
      { url: "https://wikileaks.org/ciav7p1/cms/page_12353643.html", label: "WikiLeaks" },
      { url: "https://www.consumerreports.org/electronics-computers/privacy/a-closer-look-at-the-tvs-from-the-cia-vault-7-hack-a1864416431/", label: "Consumer Reports" },
    ],
  },
  {
    entity: "cia",
    headline: "The CIA ran a covert hacking base out of a German consulate",
    explanation:
      "WikiLeaks' 'Year Zero' release revealed the CIA ran a covert hacker base out of its Frankfurt consulate, covering Europe, the Middle East, and Africa. Staff used diplomatic cover to cross Schengen borders unchecked, then used USB sticks to compromise target machines.",
    year: "2017",
    severity: "HIGH",
    sources: [
      { url: "https://www.thelocal.de/20170307/wikileaks-claims-us-frankfurt-consulate-is-a-cia-hacker-base", label: "The Local Germany" },
      { url: "https://wikileaks.org/ciav7p1/", label: "WikiLeaks Vault 7 \"Year Zero\"" },
    ],
    image: "cia-frankfurt-consulate.jpg",
  },
  {
    entity: "cia",
    headline: "Leaked CIA code showed it forging fake Kaspersky security certificates",
    explanation:
      "WikiLeaks released the source code for 'Hive,' the CIA's command-and-control backend for tasking malware implants. The files showed the CIA forging digital certificates falsely impersonating Kaspersky Lab to evade detection.",
    year: "2017",
    severity: "CRITICAL",
    sources: [
      { url: "https://thehackernews.com/2017/11/cia-hive-malware-code.html", label: "The Hacker News" },
      { url: "https://securityaffairs.com/65355/intelligence/vault-8-hive-platform.html", label: "Security Affairs" },
    ],
  },
  {
    entity: "cia",
    headline: "A US-Israeli cyberweapon physically destroyed a thousand of Iran's nuclear centrifuges",
    explanation:
      "A joint US-Israeli sabotage program deployed the Stuxnet worm against Iran's Natanz enrichment plant, destroying roughly 1,000 of 5,000 centrifuges and delaying Iran's nuclear program by an estimated 18 months to two years — the first cyberweapon known to cause physical infrastructure damage.",
    year: "2010",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.nytimes.com/2012/06/01/world/middleeast/obama-ordered-wave-of-cyberattacks-against-iran.html", label: "The New York Times" },
      { url: "https://www.npr.org/sections/thetwo-way/2012/06/01/154127061/obama-sped-up-wave-of-cyberattacks-against-iran-says-nyt", label: "NPR" },
      { url: "https://en.wikipedia.org/wiki/Operation_Olympic_Games", label: "Wikipedia" },
    ],
    image: "natanz-nuclear-facility-2006.jpg",
  },
  {
    entity: "cia",
    headline: "The CIA was Palantir's only customer for its first three years",
    explanation:
      "The CIA's venture arm rescued Palantir with two funding rounds after Silicon Valley VCs passed on the startup. From 2005 to 2008 the CIA was Palantir's patron and only customer, helping it expand into defense and commercial markets.",
    year: "2003–05",
    severity: "HIGH",
    sources: [
      { url: "https://www.forbes.com/sites/andygreenberg/2013/08/14/agent-of-intelligence-how-a-deviant-philosopher-built-palantir-a-cia-funded-data-mining-juggernaut/", label: "Forbes" },
      { url: "https://fortune.com/2025/07/29/in-q-tel-cia-venture-capital-palantir-anduril/", label: "Fortune" },
      { url: "https://en.wikipedia.org/wiki/In-Q-Tel", label: "Wikipedia" },
    ],
  },
  {
    entity: "cia",
    headline: "A Swiss encryption firm allegedly sold banks and spies rigged devices",
    explanation:
      "Swiss broadcaster SRF reported that Omnisec AG, Crypto AG's chief Swiss competitor, sold manipulated encryption devices to Swiss intelligence agencies and a major bank; a longtime consultant said NSA representatives sought influence over Omnisec's products in 1989 and were refused.",
    year: "2020",
    severity: "HIGH",
    sources: [
      { url: "https://www.swissinfo.ch/eng/business/second-swiss-firm-allegedly-sold-encrypted-spying-devices/46186432", label: "SWI swissinfo.ch" },
      { url: "https://www.securityweek.com/report-claims-cia-controlled-second-swiss-encryption-firm/", label: "SecurityWeek" },
    ],
  },

  // ---- FBI ----
  {
    entity: "fbi",
    headline: "The FBI secretly ran an encrypted phone company used by criminal gangs",
    explanation:
      "The FBI and Australian Federal Police secretly built and distributed 'Anom,' an encrypted-phone platform marketed to organized crime, with a hidden master key copying every message to FBI servers. Over 12,000 devices reached 300+ syndicates, yielding 800+ arrests.",
    year: "2018–21",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.justice.gov/usao-sdca/pr/fbi-s-encrypted-phone-platform-infiltrated-hundreds-criminal-syndicates-result-massive", label: "DOJ (Southern District of California)" },
      { url: "https://www.npr.org/2021/06/08/1004332551/drug-rings-platform-operation-trojan-shield-anom-operation-greenlight", label: "NPR" },
      { url: "https://www.fbi.gov/news/stories/fbi-global-partners-announce-results-of-operation-trojan-shield-060821", label: "FBI.gov" },
    ],
  },
  {
    entity: "fbi",
    headline: "The FBI installed a box at your ISP to sniff out targeted email",
    explanation:
      "Carnivore was an FBI system installed at an ISP to sniff all packet traffic on a network segment and filter out a target's email. The Wall Street Journal exposed its use against EarthLink in 2000; it was later abandoned for commercial tools.",
    year: "2000",
    severity: "CRITICAL",
    sources: [
      { url: "https://archive.epic.org/privacy/carnivore/", label: "EPIC" },
      { url: "https://en.wikipedia.org/wiki/Carnivore_(software)", label: "Wikipedia" },
    ],
  },
  {
    entity: "fbi",
    headline: "FBI keylogging malware waited for suspects to type their encryption password",
    explanation:
      "Magic Lantern was FBI keystroke-logging software, deployable remotely via email or an OS exploit, that activated when a suspect used PGP encryption to capture the passphrase so the FBI could decrypt seized communications.",
    year: "2001",
    severity: "HIGH",
    sources: [
      { url: "https://en.wikipedia.org/wiki/Magic_Lantern_(spyware)", label: "Wikipedia" },
      { url: "https://www.nbcnews.com/id/wbna3341694", label: "NBC News" },
    ],
  },
  {
    entity: "fbi",
    headline: "The FBI unmasked a teenage bomb-threat suspect with a fake MySpace link",
    explanation:
      "CIPAV was FBI surveillance software delivered via a deceptive MySpace link to unmask an anonymous suspect. A 2007 warrant in a bomb-threat case authorized it to record a target's IP address, running programs, and browser details.",
    year: "2007",
    severity: "HIGH",
    sources: [
      { url: "https://www.computerworld.com/article/1583582/faq-what-we-know-now-about-the-fbi-s-cipav-spyware.html", label: "Computerworld" },
      { url: "https://en.wikipedia.org/wiki/Computer_and_Internet_Protocol_Address_Verifier", label: "Wikipedia" },
    ],
  },
  {
    entity: "fbi",
    headline: "The FBI ran a seized child abuse site for two weeks to hack visitors",
    explanation:
      "After seizing the Tor-hidden child-sexual-abuse-material site Playpen, the FBI kept it running under its own control for nearly two weeks and deployed hacking code to unmask visitors' real IP addresses, infecting more than 1,000 computers worldwide.",
    year: "2015",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.eff.org/deeplinks/2016/09/playpen-story-fbis-unprecedented-and-illegal-hacking-operation", label: "EFF" },
      { url: "https://www.eff.org/pages/playpen-cases-frequently-asked-questions", label: "EFF FAQ" },
    ],
  },
  {
    entity: "fbi",
    headline: "The FBI paid hackers $900,000 to unlock an iPhone Apple wouldn't crack",
    explanation:
      "After Apple refused a 2016 court order to help unlock the San Bernardino shooter's iPhone, the FBI paid a private firm roughly $900,000 for an exploit chain to bypass the passcode limit. No actionable intelligence was recovered from the phone.",
    year: "2016",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.washingtonpost.com/technology/2021/04/14/azimuth-san-bernardino-apple-iphone-fbi/", label: "The Washington Post" },
      { url: "https://appleinsider.com/articles/21/04/14/firm-that-unlocked-san-bernardino-shooters-iphone-for-fbi-is-revealed", label: "AppleInsider" },
      { url: "https://en.wikipedia.org/wiki/FBI%E2%80%93Apple_encryption_dispute", label: "Wikipedia" },
    ],
  },
  {
    entity: "fbi",
    headline: "The FBI secretly bought and tested NSO's Pegasus spyware in New Jersey",
    explanation:
      "The FBI secretly bought NSO Group's Pegasus spyware in 2019, testing it at a walled-off New Jersey facility with dummy accounts and foreign SIM cards. After two years of deliberation, the FBI decided against operational use in 2021.",
    year: "2019",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.washingtonpost.com/technology/2022/02/02/pegasus-fbi-nso-test/", label: "The Washington Post" },
      { url: "https://9to5mac.com/2022/01/28/us-version-of-pegasus-fbi/", label: "9to5Mac" },
      { url: "https://epic.org/report-fbi-explored-using-spyware-pegasus-for-criminal-investigations/", label: "EPIC" },
    ],
  },
  {
    entity: "fbi",
    headline: "AT&T employees embedded with police search phone records back to 1987",
    explanation:
      "Hemisphere embeds AT&T employees with DEA, FBI, and other agents to search call-detail records dating to 1987, adding roughly 4 billion new records daily. Agencies were instructed to conceal its use via 'parallel construction.'",
    year: "2007–Ongoing",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.eff.org/cases/hemisphere", label: "EFF" },
      { url: "https://archive.epic.org/2018/09/epic-foia-docs-show-fbi-and-cb.html", label: "EPIC" },
      { url: "https://www.aclu.org/news/national-security/vast-troubling-call-database-drug-agents-use", label: "ACLU" },
    ],
  },
  {
    entity: "fbi",
    headline: "The FBI director demanded a law forcing backdoors into every phone",
    explanation:
      "FBI Director James Comey argued in a 2014 speech that default device encryption from Apple and Google would let criminals evade lawful court-ordered searches, launching a multi-year FBI push for legislated encryption backdoors.",
    year: "2014",
    severity: "HIGH",
    sources: [
      { url: "https://www.fbi.gov/news/speeches-and-testimony/going-dark-are-technology-privacy-and-public-safety-on-a-collision-course", label: "FBI" },
      { url: "https://www.washingtonpost.com/news/the-switch/wp/2014/10/17/fbi-director-comey-calls-on-congress-to-stop-unlockable-encryption-good-luck-with-that/", label: "The Washington Post" },
      { url: "https://www.brookings.edu/articles/watch-fbi-director-james-comey-on-technology-law-enforcement-and-going-dark/", label: "Brookings" },
    ],
  },

  // ---- RSA ----
  {
    entity: "rsa",
    headline: "The NSA paid RSA $10 million to default to broken encryption",
    explanation:
      "Reuters reported the NSA paid RSA Security $10 million to make the likely-backdoored Dual_EC_DRBG the default random-number generator in its widely used BSAFE toolkit starting in 2004, before NIST even standardized the algorithm.",
    year: "2004–13",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.theregister.com/2013/12/21/nsa_paid_rsa_10_million/", label: "The Register" },
      { url: "https://www.geekwire.com/2013/report-rsa-10m-nsa-push-bad-crypto/", label: "GeekWire" },
      { url: "https://www.huffpost.com/2013/12/20/nsa-rsa-contract_n_4482227.html", label: "HuffPost" },
    ],
  },
  {
    entity: "rsa",
    headline: "Researchers found RSA's default encryption had a built-in skeleton key",
    explanation:
      "Microsoft researchers showed at a 2007 conference that Dual_EC_DRBG's constants could work as a mathematical skeleton key, letting whoever chose them predict all future outputs. RSA kept it as BSafe's default until 2013, when Snowden leaks confirmed the NSA's role.",
    year: "2007",
    severity: "HIGH",
    sources: [
      { url: "https://www.wired.com/2013/09/nsa-backdoor/", label: "Wired" },
      { url: "https://arstechnica.com/security/2013/12/nsa-influenced-crypto-standard-may-have-poisoned-rsas-bsafe-tool-after-all/", label: "Ars Technica" },
    ],
  },

  // ---- Juniper Networks ----
  {
    entity: "juniper",
    headline: "A hidden master password let anyone log into Juniper firewalls as admin",
    explanation:
      "In 2015 Juniper disclosed 'unauthorized code' in its NetScreen firewall software: a hardcoded master password letting any attacker gain admin access, found by a researcher within six hours, plus a separate flaw enabling passive VPN traffic decryption.",
    year: "2015",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.rapid7.com/blog/post/2015/12/20/cve-2015-7755-juniper-screenos-authentication-backdoor/", label: "Rapid7" },
      { url: "https://www.securityweek.com/juniper-firewall-backdoor-password-found-6-hours/", label: "SecurityWeek" },
    ],
  },
  {
    entity: "juniper",
    headline: "Someone hijacked an NSA-style backdoor in Juniper VPNs for their own spying",
    explanation:
      "Researchers found the VPN-decryption backdoor worked by altering an encryption constant Juniper itself had introduced in 2008 — suggesting an outside actor repurposed NSA-linked groundwork for its own espionage. Congress pressed Juniper's CEO in 2020 after it still hadn't named a responsible party.",
    year: "2008–20",
    severity: "HIGH",
    sources: [
      { url: "https://www.schneier.com/blog/archives/2016/04/details_about_j.html", label: "Schneier on Security" },
      { url: "https://www.theregister.com/2020/06/10/congress_juniper_letter/", label: "The Register" },
    ],
  },

  // ---- Cisco ----
  {
    entity: "cisco",
    headline: "The NSA opened shipped Cisco routers to implant hidden beacons",
    explanation:
      "Leaked NSA photos showed staff opening a shipped Cisco box to implant a beacon before resealing and forwarding it to the customer. Cisco's CEO wrote to President Obama warning the practice would undermine confidence in US tech.",
    year: "2014",
    severity: "CRITICAL",
    sources: [
      { url: "https://techcrunch.com/2014/05/18/the-nsa-cisco-and-the-issue-of-interdiction/", label: "TechCrunch" },
      { url: "https://www.computerworld.com/article/1633983/to-avoid-nsa-cisco-delivers-gear-to-strange-addresses.html", label: "Computerworld" },
    ],
  },
  {
    entity: "cisco",
    headline: "Cisco's built-in wiretap feature could be hijacked by any insider",
    explanation:
      "A 2010 Black Hat presentation showed Cisco's built-in lawful-intercept architecture let unauthorized insiders activate wiretaps, discover existing surveillance targets, or disable the audit trail, with no lockout on failed logins.",
    year: "2010",
    severity: "HIGH",
    sources: [
      { url: "https://www.forbes.com/2010/02/03/hackers-networking-equipment-technology-security-cisco.html", label: "Forbes" },
      { url: "https://blackhat.com/presentations/bh-dc-10/Cross_Tom/BlackHat-DC-2010-Cross-Attacking-LawfulI-Intercept-wp.pdf", label: "Black Hat (IBM X-Force whitepaper)" },
    ],
  },
  {
    entity: "cisco",
    headline: "A leaked NSA catalog listed permanent backdoor firmware for Cisco firewalls",
    explanation:
      "A leaked NSA hardware/firmware implant catalog listed 'JETPLOW,' firmware giving a permanent backdoor to Cisco PIX and ASA firewalls that persisted across reboots at zero additional cost.",
    year: "2013",
    severity: "CRITICAL",
    sources: [
      { url: "https://www.eff.org/files/2014/01/06/20131230-appelbaum-nsa_ant_catalog.pdf", label: "EFF (leaked NSA ANT catalog document)" },
      { url: "https://en.wikipedia.org/wiki/NSA_ANT_catalog", label: "Wikipedia (secondary corroboration)" },
    ],
    image: "nsa-ant-jetplow.jpg",
  },
];

/** Orbiting tech-giant nodes, in display order. */
export const TECH_ORDER = ["nvidia","google","microsoft","meta","amazon","apple","openai","zoom","oracle","adobe","slack","rsa","juniper","cisco"];

/** Central intel/state cluster; the middle entry renders larger. */
export const AGENCY_ORDER = ["nsa","usgov","cia","fbi"];

/** Consumer "perceived-secure vs actually-provable" exposures — the everyday
 *  beliefs that don't survive a forensic examiner. A distinct ledger from the
 *  historical big-tech/agency EXPOSURES above; `entity` here is a topic slug,
 *  not an ENTITIES node, so these are not rendered as matrix nodes. Every claim
 *  is grounded in the sources listed. Keep headlines accurate — nuance beats a
 *  false absolute (e.g. an iPhone is strong in BFU, weak AFU/backup, not
 *  simply "unhackable" or "forensic-proof"). */
export const PERCEPTIONS: Exposure[] = [
  {
    entity: "iphone",
    headline: "Your iPhone is a vault only while it's switched off",
    explanation:
      "Before First Unlock (powered off, passcode never entered since boot) most keys are sealed and even Cellebrite/GrayKey struggle. But phones are usually seized After First Unlock, where far more is extractable — and without Advanced Data Protection, an iCloud backup hands Apple, and any warrant, the same data anyway.",
    year: "Ongoing",
    severity: "HIGH",
    sources: [
      { url: "https://www.404media.co/leaked-documents-show-what-phones-secretive-tech-graykey-can-unlock-2/", label: "404 Media" },
      { url: "https://www.macrumors.com/2024/11/19/graykey-ios-18-partial-unlock/", label: "MacRumors" },
      { url: "https://support.apple.com/guide/security/advanced-data-protection-for-icloud-sec973254c5f/web", label: "Apple" },
    ],
  },
  {
    entity: "android",
    headline: "Samsung Knox is paint over stock Android — a real secure phone seals its keys in hardware",
    explanation:
      "Knox/Defex/RKP are app-layer restrictions forensic vendors have built bypasses around (Magnet lists physical acquisition for 1,300+ Samsung models). GrapheneOS on a Pixel seals keys in the Titan M2 element, auto-reboots to BFU, and offers a duress PIN that instantly wipes — the architecture behind secureOS/Privon.",
    year: "Ongoing",
    severity: "HIGH",
    sources: [
      { url: "https://www.magnetforensics.com/blog/loading-cellebrite-images-into-magnet-axiom/", label: "Magnet Forensics" },
      { url: "https://grapheneos.org/faq", label: "GrapheneOS" },
    ],
  },
  {
    entity: "bitlocker",
    headline: "BitLocker being on is not the same as safe — the default unlocks itself for a thief with a USB stick",
    explanation:
      "Consumer BitLocker defaults to TPM-only auto-unlock with no PIN. The bitpixie / CVE-2025-48804 class feeds the boot manager a signed-but-manipulated recovery environment to pull the key without a PIN — demonstrated bypassing TPM-only Windows 11 in under five minutes. Enabling TPM+PIN defeats the entire attack class.",
    year: "2023-2025",
    severity: "HIGH",
    sources: [
      { url: "https://blog.compass-security.com/2025/05/bypassing-bitlocker-encryption-bitpixie-poc-and-winpe-edition/", label: "Compass Security" },
      { url: "https://blog.elcomsoft.com/2026/05/a-decade-of-bitlocker-vulnerabilities-whats-patched-whats-not-and-what-still-works/", label: "Elcomsoft" },
      { url: "https://www.techpowerup.com/348954/bitunlocker-downgrade-attack-bypasses-tpm-only-windows-11-bitlocker-in-under-5-minutes", label: "TechPowerUp" },
    ],
  },
  {
    entity: "messaging",
    headline: "End-to-end encryption hides what you said, not who you talked to — or what's on the phone",
    explanation:
      "E2EE protects message content in transit. It does nothing for metadata (WhatsApp can yield contacts and who-messaged-whom-when to a warrant or pen register) or for a seized, unlocked device, where the plaintext message database is extractable regardless of which protocol carried it.",
    year: "Ongoing",
    severity: "HIGH",
    sources: [
      { url: "https://www.justsecurity.org/79549/we-now-know-what-information-the-fbi-can-obtain-from-encrypted-messaging-apps/", label: "Just Security" },
      { url: "https://faq.whatsapp.com/444002211197967", label: "WhatsApp Law Enforcement guidelines" },
    ],
  },
  {
    entity: "deletion",
    headline: "Deleting a file doesn't delete it — and on an SSD, neither does overwriting it",
    explanation:
      "Delete, empty-trash, and factory-reset normally remove only the pointer, leaving the data carvable. On flash (SSD/NVMe/eMMC) wear-leveling means an OS-level overwrite often misses the physical cells — NIST SP 800-88 treats overwrite as unreliable on flash and elevates cryptographic erase as the dependable method.",
    year: "Ongoing",
    severity: "HIGH",
    sources: [
      { url: "https://nvlpubs.nist.gov/nistpubs/SpecialPublications/NIST.SP.800-88r2.pdf", label: "NIST SP 800-88 Rev. 2" },
      { url: "https://www.sciencedirect.com/science/article/pii/S2666281723000963", label: "ScienceDirect (Android reset study)" },
    ],
  },
  {
    entity: "cloud-account",
    headline: "Your Google account isn't yours — it answers to warrants and can be switched off without you",
    explanation:
      "A strong password doesn't change two facts: providers field tens of thousands of law-enforcement requests (Google received ~40,000 in the first half of 2020 alone) and can suspend or terminate an account unilaterally. For a business run on one Workspace identity that's a continuity risk, not just a privacy one — custody, not control.",
    year: "Ongoing",
    severity: "HIGH",
    sources: [
      { url: "https://transparencyreport.google.com/user-data/overview", label: "Google Transparency Report" },
      { url: "https://support.google.com/legal-help-center/answer/16719647?hl=en", label: "Google Legal Help" },
    ],
  },
  {
    entity: "physical-capture",
    headline: "The leak no security tool sees: someone points a phone at the screen and walks out",
    explanation:
      "A photo of the screen generates no file transfer, download, or log — so conventional DLP, which watches data flows, is blind to it. It's a documented, common insider move. Only least-data-on-screen defaults, watermarking/session monitoring, and camera-discipline policy address it — no amount of encryption does.",
    year: "Ongoing",
    severity: "HIGH",
    sources: [
      { url: "https://www.echomark.com/post/the-insider-with-a-camera-phone-the-leak-vector-no-security-tool-sees-coming", label: "EchoMark" },
      { url: "https://www.dtex.ai/blog/modern-data-exfiltration-patterns/", label: "DTEX" },
      { url: "https://www.strongdm.com/what-is/shoulder-surfing", label: "StrongDM" },
    ],
  },
];
