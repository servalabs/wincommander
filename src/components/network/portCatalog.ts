// Common-port → friendly-name catalog. Covers the protocols most users will
// recognise; falls back to null for ephemeral / private ports. Kept compact
// on purpose — IANA's full list is huge and most of it is irrelevant noise.
export const PORT_SERVICE_NAMES: Record<number, string> = {
    20: "FTP",
    21: "FTP",
    22: "SSH",
    23: "Telnet",
    25: "SMTP",
    53: "DNS",
    67: "DHCP",
    68: "DHCP",
    69: "TFTP",
    80: "HTTP",
    88: "Kerberos",
    110: "POP3",
    111: "RPC",
    119: "NNTP",
    123: "NTP",
    135: "RPC",
    137: "NetBIOS",
    138: "NetBIOS",
    139: "NetBIOS",
    143: "IMAP",
    161: "SNMP",
    162: "SNMP",
    179: "BGP",
    194: "IRC",
    389: "LDAP",
    427: "SLP",
    443: "HTTPS",
    445: "SMB",
    465: "SMTPS",
    500: "IPsec",
    514: "Syslog",
    515: "LPD",
    520: "RIP",
    554: "RTSP",
    587: "SMTP-TLS",
    593: "RPC-HTTP",
    631: "IPP",
    636: "LDAPS",
    873: "rsync",
    902: "VMware",
    989: "FTPS",
    990: "FTPS",
    993: "IMAPS",
    995: "POP3S",
    1080: "SOCKS",
    1194: "OpenVPN",
    1433: "MSSQL",
    1434: "MSSQL",
    1521: "Oracle",
    1701: "L2TP",
    1723: "PPTP",
    1812: "RADIUS",
    1813: "RADIUS",
    1883: "MQTT",
    1900: "SSDP",
    2049: "NFS",
    2375: "Docker",
    2376: "Docker-TLS",
    2379: "etcd",
    2483: "Oracle",
    3000: "Dev",
    3128: "Squid",
    3306: "MySQL",
    3389: "RDP",
    3478: "STUN",
    3702: "WS-Discovery",
    4500: "IPsec-NAT",
    5000: "Dev",
    5060: "SIP",
    5061: "SIP-TLS",
    5173: "Vite",
    5222: "XMPP",
    5269: "XMPP",
    5353: "mDNS",
    5355: "LLMNR",
    5432: "PostgreSQL",
    5672: "AMQP",
    5683: "CoAP",
    5800: "VNC-HTTP",
    5900: "VNC",
    5938: "TeamViewer",
    6379: "Redis",
    6443: "K8s API",
    6881: "BitTorrent",
    8000: "HTTP-alt",
    8080: "HTTP-alt",
    8443: "HTTPS-alt",
    8883: "MQTT-TLS",
    9000: "Dev",
    9090: "Prometheus",
    9092: "Kafka",
    9100: "Printer",
    9200: "Elastic",
    9418: "Git",
    11211: "Memcached",
    27017: "MongoDB",
    27018: "MongoDB",
    27019: "MongoDB",
    32400: "Plex",
    41641: "Private Mesh",
    47821: "WC Webhook",
    51820: "WireGuard",
};

export function getServiceName(port: number): string | null {
    return PORT_SERVICE_NAMES[port] ?? null;
}

export type PortDirection = "listen" | "incoming" | "outgoing" | "closing" | "pending";

const SERVER_PORT_HEURISTIC_MAX = 1024;

export interface DirectionInput {
    proto: "TCP" | "UDP";
    state: string;
    localPort: number;
    remotePort: number;
}

// Infer direction from state + port pair.
//
//   - LISTEN sockets are servers waiting for inbound connections.
//   - For ESTABLISHED, the side whose port is "well-known" (under 1024 OR a
//     known service port) is the server. If our local port is well-known,
//     someone connected IN; otherwise we connected OUT.
//   - Closing-state TCP sockets get a dedicated icon so the user can spot
//     stale activity.
//   - UDP is per-endpoint, no real "direction" — always LISTEN by definition.
export function inferDirection({ proto, state, localPort, remotePort }: DirectionInput): PortDirection {
    if (proto === "UDP") return "listen";
    const s = state.toUpperCase();
    if (s === "LISTEN") return "listen";
    if (s === "BOUND") return "pending";
    if (
        s === "TIME_WAIT" || s === "CLOSE_WAIT" || s === "CLOSING" ||
        s === "LAST_ACK" || s === "FIN_WAIT1" || s === "FIN_WAIT2"
    ) return "closing";

    // ESTABLISHED, SYN_SENT, SYN_RCVD — try to attribute direction.
    const localIsKnown = localPort > 0 && localPort <= SERVER_PORT_HEURISTIC_MAX;
    const remoteIsKnown = remotePort > 0 && remotePort <= SERVER_PORT_HEURISTIC_MAX;
    const localIsService = !!PORT_SERVICE_NAMES[localPort];
    const remoteIsService = !!PORT_SERVICE_NAMES[remotePort];

    if (localIsKnown || localIsService) return "incoming";
    if (remoteIsKnown || remoteIsService) return "outgoing";
    return "outgoing";
}

export function isLoopback(addr: string): boolean {
    return addr === "127.0.0.1" || addr === "::1" || addr.startsWith("127.");
}
