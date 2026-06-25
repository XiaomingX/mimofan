import { ImageResponse } from "next/og";
import { IDENTITY_PHRASE, SITE_NAME } from "@/lib/page-meta";

export const alt = `${SITE_NAME} — ${IDENTITY_PHRASE}`;
export const size = { width: 1200, height: 630 };
export const contentType = "image/png";

// Palette mirrors tailwind.config.ts: ink #0E0E10, indigo #4D6BFE,
// paper-line-soft #D4D8E2, ink-mute #6B7280.
export default function OpengraphImage() {
  return new ImageResponse(
    (
      <div
        style={{
          width: "100%",
          height: "100%",
          display: "flex",
          flexDirection: "column",
          justifyContent: "space-between",
          backgroundColor: "#0E0E10",
          padding: "72px 84px",
          fontFamily: "sans-serif",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 16,
            fontSize: 26,
            letterSpacing: 0,
            textTransform: "uppercase",
            color: "#6B7280",
          }}
        >
          <div style={{ width: 14, height: 14, backgroundColor: "#4D6BFE" }} />
          codewhale.net
        </div>
        <div style={{ display: "flex", flexDirection: "column" }}>
          <div
            style={{
              fontSize: 116,
              fontWeight: 700,
              color: "#FFFFFF",
              letterSpacing: 0,
            }}
          >
            CodeWhale
          </div>
          <div
            style={{
              marginTop: 28,
              fontSize: 38,
              lineHeight: 1.35,
              color: "#D4D8E2",
              maxWidth: 980,
            }}
          >
            {IDENTITY_PHRASE}
          </div>
        </div>
        <div style={{ display: "flex", width: "100%", height: 6, backgroundColor: "#4D6BFE" }} />
      </div>
    ),
    { ...size },
  );
}
