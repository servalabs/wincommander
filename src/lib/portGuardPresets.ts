// SPDX-License-Identifier: AGPL-3.0-or-later
import type { PortProtocol } from './portGuard';

export interface PortPreset { name: string; ports: string; protocol: PortProtocol }
export interface PortPresetCategory { name: string; presets: PortPreset[] }

export const PORT_PRESET_CATEGORIES: PortPresetCategory[] = [
  { name: 'Remote access', presets: [
  { name: 'RDP', ports: '3389', protocol: 'both' },
  { name: 'SSH / SFTP', ports: '22', protocol: 'tcp' },
  { name: 'Telnet', ports: '23', protocol: 'tcp' },
  { name: 'VNC', ports: '5900', protocol: 'tcp' },
  { name: 'TeamViewer', ports: '5938', protocol: 'both' },
  { name: 'AnyDesk relay', ports: '80,443,6568', protocol: 'tcp' },
  { name: 'AnyDesk discovery', ports: '50001-50003', protocol: 'udp' },
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
  { name: 'NetBIOS', ports: '137,138,139', protocol: 'both' },
  { name: 'FTP', ports: '21', protocol: 'tcp' },
  { name: 'FTP-Data', ports: '20', protocol: 'tcp' },
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
  { name: 'Steam', ports: '27015,27036', protocol: 'both' },
  { name: 'Xbox-Live', ports: '3074', protocol: 'both' },
  { name: 'PlayStation', ports: '1935,3478,3479', protocol: 'both' },
  { name: 'Epic-Games', ports: '5222', protocol: 'tcp' },
  { name: 'Battle.net', ports: '1119', protocol: 'both' },
  { name: 'Minecraft', ports: '25565', protocol: 'tcp' },
  ] },
  { name: 'Meetings', presets: [
  { name: 'Zoom', ports: '8801,8802', protocol: 'both' },
  { name: 'Teams', ports: '3478,3479,3480', protocol: 'udp' },
  ] },
  { name: 'Torrent / P2P', presets: [
  { name: 'BitTorrent', ports: '6881-6889', protocol: 'both' },
  { name: 'uTorrent', ports: '6969', protocol: 'both' },
  ] },
  { name: 'IoT', presets: [
  { name: 'MQTT', ports: '1883', protocol: 'tcp' },
  { name: 'MQTT-TLS', ports: '8883', protocol: 'tcp' },
  { name: 'CoAP', ports: '5683', protocol: 'udp' },
  ] },
];

export const PORT_PRESETS = PORT_PRESET_CATEGORIES.flatMap(category => category.presets);
