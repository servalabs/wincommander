// SPDX-License-Identifier: AGPL-3.0-or-later
import type { PortProtocol } from './portGuard';

export interface PortPreset { name: string; ports: string; protocol: PortProtocol; note?: string }
export interface PortPresetCategory { name: string; presets: PortPreset[] }

export const PORT_PRESET_CATEGORIES: PortPresetCategory[] = [
  { name: 'Remote access', presets: [
  { name: 'RDP', ports: '3389', protocol: 'both' },
  { name: 'SSH / SFTP', ports: '22', protocol: 'tcp' },
  { name: 'Telnet', ports: '23', protocol: 'tcp' },
  { name: 'VNC', ports: '5900', protocol: 'tcp' },
  { name: 'AnyDesk direct', ports: '7070', protocol: 'tcp', note: 'Default direct-connection port. Check AnyDesk Settings > Connection if changed. Relay-only sessions do not arrive on this port.' },
  { name: 'TeamViewer LAN', ports: '5938', protocol: 'both', note: 'For direct LAN connections when enabled in TeamViewer. Internet sessions may use outbound relays or dynamic ports, so this is not a complete session detector.' },
  { name: 'AnyDesk discovery', ports: '50001-50003', protocol: 'udp', note: 'LAN discovery only; this does not identify a remote-control session.' },
  ] },
  { name: 'Web', presets: [
  { name: 'HTTP', ports: '80', protocol: 'tcp' },
  { name: 'HTTPS', ports: '443', protocol: 'both' },
  { name: 'HTTP-Alt', ports: '8080', protocol: 'tcp' },
  { name: 'HTTPS-Alt', ports: '8443', protocol: 'tcp' },
  { name: 'HTTP-Dev', ports: '3000', protocol: 'tcp' },
  { name: 'HTTP-Dev2', ports: '5173', protocol: 'tcp' },
  ] },
  { name: 'File sharing', presets: [
  { name: 'SMB', ports: '445', protocol: 'tcp' },
  { name: 'NetBIOS name', ports: '137', protocol: 'both' },
  { name: 'NetBIOS datagram', ports: '138', protocol: 'udp' },
  { name: 'NetBIOS session', ports: '139', protocol: 'tcp' },
  { name: 'FTP', ports: '21', protocol: 'tcp' },
  ] },
  { name: 'Database', presets: [
  { name: 'MySQL', ports: '3306', protocol: 'tcp' },
  { name: 'PostgreSQL', ports: '5432', protocol: 'tcp' },
  { name: 'MSSQL', ports: '1433', protocol: 'tcp' },
  { name: 'Redis', ports: '6379', protocol: 'tcp' },
  { name: 'MongoDB', ports: '27017', protocol: 'tcp' },
  { name: 'Elasticsearch', ports: '9200', protocol: 'tcp' },
  ] },
  { name: 'Mail', presets: [
  { name: 'SMTP', ports: '25', protocol: 'tcp' },
  { name: 'SMTP-TLS', ports: '587', protocol: 'tcp' },
  { name: 'SMTPS', ports: '465', protocol: 'tcp' },
  { name: 'POP3S', ports: '995', protocol: 'tcp' },
  { name: 'IMAPS', ports: '993', protocol: 'tcp' },
  ] },
  { name: 'Gaming', presets: [
  { name: 'Source server', ports: '27015', protocol: 'both' },
  { name: 'Steam Remote Play TCP', ports: '27036', protocol: 'tcp' },
  { name: 'Steam Remote Play UDP', ports: '27031-27036', protocol: 'udp' },
  { name: 'Xbox-Live', ports: '3074', protocol: 'both' },
  { name: 'Minecraft Java server', ports: '25565', protocol: 'tcp' },
  ] },
  { name: 'Torrent / P2P', presets: [
  { name: 'BitTorrent legacy range', ports: '6881-6889', protocol: 'both', note: 'BitTorrent and uTorrent can use a custom or randomized listening port. Enter the actual incoming port shown in your torrent app; 6969 is not a universal uTorrent port.' },
  ] },
  { name: 'IoT', presets: [
  { name: 'MQTT', ports: '1883', protocol: 'tcp' },
  { name: 'MQTT-TLS', ports: '8883', protocol: 'tcp' },
  { name: 'CoAP', ports: '5683', protocol: 'udp' },
  ] },
];

export const PORT_PRESETS = PORT_PRESET_CATEGORIES.flatMap(category => category.presets);
