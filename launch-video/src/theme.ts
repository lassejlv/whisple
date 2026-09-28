import { loadFont } from "@remotion/google-fonts/Inter";

export const { fontFamily } = loadFont("normal", {
  weights: ["400", "500", "600", "700"],
  subsets: ["latin"],
});

// Mirrors src/ui/theme.rs so the film matches the app.
export const color = {
  hud: "#0c0c0e",
  inset: "#17171a",
  raised: "#232327",
  hairline: "rgba(255,255,255,0.08)",
  label: "#f5f5f7",
  secondary: "#98989f",
  tertiary: "#5e5e65",
  amber: "#ffb340",
  amberSoft: "#2a1f0c",
};

export const ease = {
  out: [0.16, 1, 0.3, 1] as const,
  inOut: [0.65, 0, 0.35, 1] as const,
};
