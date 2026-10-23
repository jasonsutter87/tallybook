// Appearance card: light, dark or follow the system.

import { Check, Monitor, Moon, Palette, Sun } from "lucide-react";
import { useState, type ReactNode } from "react";
import { readTheme, saveTheme, type ThemeChoice } from "../../theme";
import { SettingsCard } from "./shared";

export default function AppearanceCard() {
  const [theme, setTheme] = useState<ThemeChoice>(readTheme);
  const choices: { value: ThemeChoice; label: string; icon: ReactNode; swatch: [string, string][] }[] = [
    {
      value: "system",
      label: "System",
      icon: <Monitor size={15} />,
      swatch: [
        ["#f3f5f9", "#d5dbe6"],
        ["#0a1220", "#2a3a54"],
      ],
    },
    { value: "light", label: "Light", icon: <Sun size={15} />, swatch: [["#f3f5f9", "#d5dbe6"]] },
    { value: "dark", label: "Dark", icon: <Moon size={15} />, swatch: [["#0a1220", "#2a3a54"]] },
  ];
  return (
    <SettingsCard id="appearance" icon={<Palette size={18} />} title="Appearance" desc="Saved on this computer. System follows your device's light or dark setting.">
      <div className="theme-choices" role="radiogroup" aria-label="Theme">
        {choices.map((c) => (
          <button
            key={c.value}
            role="radio"
            aria-checked={theme === c.value}
            className={`theme-choice ${theme === c.value ? "active" : ""}`}
            onClick={() => {
              setTheme(c.value);
              saveTheme(c.value);
            }}
          >
            <div className="theme-swatch">
              {c.swatch.map(([bg, bar], i) => (
                <div key={i} style={{ background: bg }}>
                  <i style={{ background: "#0aa2c0", width: "45%" }} />
                  <i style={{ background: bar, width: "80%" }} />
                  <i style={{ background: bar, width: "60%" }} />
                </div>
              ))}
            </div>
            <span className="row-between">
              <span className="row" style={{ gap: 6 }}>
                {c.icon} {c.label}
              </span>
              {theme === c.value && <Check size={15} style={{ color: "var(--brand)" }} />}
            </span>
          </button>
        ))}
      </div>
    </SettingsCard>
  );
}
