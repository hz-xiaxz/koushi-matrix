/**
 * Shared rendered-appearance probes for the mention pill (#874, #1328).
 *
 * The class and the `data-mention-user-id` attribute alone cannot show whether a
 * pill reads as a pill: in a formatted body the pill is the `<a>` itself, so the
 * formatted-body link rule can underline it and its fill can be
 * indistinguishable from the surface behind it (#1328). These probes read the
 * paint the user sees, not the markup that produced it.
 */

import type { Locator } from "@playwright/test";

/** The largest per-channel distance between two opaque colours. */
export function channelDelta(fill: number[], surface: number[]): number {
  return Math.max(...fill.slice(0, 3).map((channel, index) => Math.abs(channel - surface[index]!)));
}

/**
 * The pill's own paint plus the first surface painted behind it. Rows and panes
 * paint on ancestors, so the reference colour walks up from the pill.
 */
export async function pillAppearance(pill: Locator) {
  return (await pill.evaluate((element) => {
    const parse = (value: string) => {
      const scale = value.startsWith("color(") ? 255 : 1;
      return (value.match(/[\d.]+%?/g) ?? [])
        .map((token) => (token.endsWith("%") ? parseFloat(token) * 2.55 : parseFloat(token)))
        .map((channel) => channel * scale);
    };
    const surfaceBehind = (node: Element | null) => {
      for (let current = node?.parentElement ?? null; current; current = current.parentElement) {
        const background = getComputedStyle(current).backgroundColor;
        const channels = parse(background);
        if (channels.length >= 3 && (channels.length < 4 || channels[3]! > 0)) {
          return { background, channels: channels.slice(0, 3) };
        }
      }
      return { background: "", channels: [] as number[] };
    };
    const style = getComputedStyle(element);
    const behind = surfaceBehind(element);
    return {
      text: element.textContent ?? "",
      decoration: style.textDecorationLine,
      background: style.backgroundColor,
      backgroundChannels: parse(style.backgroundColor).slice(0, 3),
      surface: behind.background,
      surfaceChannels: behind.channels
    };
  })) as {
    text: string;
    decoration: string;
    background: string;
    backgroundChannels: number[];
    surface: string;
    surfaceChannels: number[];
  };
}
