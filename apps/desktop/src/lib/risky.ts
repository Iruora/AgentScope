import shared from "./risky-patterns.json" with { type: "json" };

/** Commands that deserve a second look before approving from AgentScope. */
const RISKY: RegExp[] = shared.patterns.map((p) => new RegExp(p, "i"));

export function isRisky(summary: string): boolean {
  return RISKY.some((re) => re.test(summary));
}
