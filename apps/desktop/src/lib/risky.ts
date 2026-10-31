/** Commands that deserve a second look before approving from AgentScope. */
const RISKY: RegExp[] = [
  /\brm\s+(-[a-z]*r[a-z]*f|-[a-z]*f[a-z]*r|--recursive|--force)\b/i,
  /\bgit\s+push\b.*(--force\b|-f\b|--force-with-lease\b|\+\w)/i,
  /\bgit\s+(reset\s+--hard|clean\s+-[a-z]*f)/i,
  /\b(curl|wget)\b[^|]*\|\s*(sudo\s+)?(ba|z)?sh\b/i,
  /\bsudo\b/i,
  /\b(mkfs|dd\s+if=|shutdown|reboot)\b/i,
  /\bchmod\s+(-R\s+)?777\b/i,
  /\b(drop\s+(table|database)|truncate\s+table)\b/i,
  /\bnpm\s+publish\b|\bcargo\s+publish\b/i,
  /\bterraform\s+(apply|destroy)\b|\bkubectl\s+delete\b/i,
];

export function isRisky(summary: string): boolean {
  return RISKY.some((re) => re.test(summary));
}
