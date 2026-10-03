// Settings > Appearance: mode plus separate light and dark palette picks.
// Clicking a swatch applies it instantly; choices are saved right away.
import { Check } from "lucide-react";
import { useEffect } from "react";
import { api } from "../lib/api";
import { errorMessage } from "../lib/actions";
import { toast } from "../lib/store";
import { DEFAULT_DARK, DEFAULT_LIGHT, PALETTES, type Palette, type Scheme } from "../theme/palettes";
import { activePalette, baseScheme, getTheme, pickPalette, setTheme, useTheme, type ThemeMode } from "../theme/theme";
import { Card, Pill, Segmented } from "./ui";

function persist() {
  const t = getTheme();
  api.saveAppearance(t.mode, t.light, t.dark).catch((e) => toast(`Couldn't save appearance: ${errorMessage(e)}`, { tone: "attention" }));
}

export function AppearanceSettings() {
  const theme = useTheme();
  const base = baseScheme(theme);

  // A preview of the inactive scheme only lasts while you're here.
  useEffect(() => () => setTheme({ preview: null }), []);

  const chooseMode = (mode: ThemeMode) => {
    setTheme({ mode, preview: null });
    persist();
  };

  const choosePalette = (p: Palette) => {
    // Applies instantly; a palette for the scheme that isn't showing is previewed.
    setTheme(pickPalette(theme, p));
    persist();
  };

  const previewing = theme.preview ? activePalette(theme) : null;
  const whenUsed = (scheme: Scheme) =>
    theme.mode === "system" ? `when Windows is in ${scheme} mode` : `when the mode is ${scheme === "dark" ? "Dark" : "Light"}`;

  return (
    <Card title="Appearance" subtitle="Changes apply instantly and are saved automatically.">
      <div className="flex flex-wrap items-center gap-3">
        <span className="text-[13px] font-medium">Mode</span>
        <Segmented<ThemeMode>
          value={theme.mode}
          onChange={chooseMode}
          options={[
            { value: "system", label: "Match Windows" },
            { value: "light", label: "Light" },
            { value: "dark", label: "Dark" },
          ]}
        />
        {theme.mode === "system" && <span className="text-xs text-ink-3">Windows is in {theme.osDark ? "dark" : "light"} mode right now.</span>}
      </div>

      {previewing && (
        <div className="mt-4 flex items-center gap-3 rounded-lg border border-line bg-accent-soft px-3 py-2 text-xs">
          <span className="flex-1 text-ink">
            Previewing {previewing.name}. Atlas uses it {whenUsed(previewing.scheme)}.
          </span>
          <button type="button" className="font-medium text-accent hover:underline" onClick={() => setTheme({ preview: null })}>
            Stop preview
          </button>
        </div>
      )}

      {(["light", "dark"] as const).map((scheme) => (
        <section key={scheme} className="mt-5">
          <div className="mb-2 flex items-center gap-2">
            <h3 className="text-[13px] font-medium">{scheme === "light" ? "Light palette" : "Dark palette"}</h3>
            {base === scheme && !theme.preview && <Pill tone="accent">In use</Pill>}
            <span className="text-xs text-ink-3">Used {whenUsed(scheme)}.</span>
          </div>
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-5">
            {PALETTES.filter((p) => p.scheme === scheme).map((p) => (
              <Swatch key={p.id} p={p} selected={(scheme === "dark" ? theme.dark : theme.light) === p.id} onPick={() => choosePalette(p)} />
            ))}
          </div>
        </section>
      ))}
    </Card>
  );
}

/** Mini preview drawn entirely in the palette's own tokens. */
function Swatch({ p, selected, onPick }: { p: Palette; selected: boolean; onPick: () => void }) {
  const t = p.tokens;
  const isDefault = p.id === DEFAULT_LIGHT || p.id === DEFAULT_DARK;
  return (
    <button
      type="button"
      onClick={onPick}
      aria-pressed={selected}
      title={`${p.name}: ${p.mood}`}
      className={`rounded-xl border p-1.5 text-left transition-colors duration-100 ${
        selected ? "border-accent ring-2 ring-[var(--accent)]" : "border-line hover:border-line-strong"
      }`}
    >
      <div className="rounded-lg border p-2" style={{ background: t.bg, borderColor: t.border }}>
        <div className="flex h-[58px] flex-col gap-1.5 rounded-md border p-2" style={{ background: t.surface, borderColor: t.border }}>
          <div className="flex items-center gap-1.5">
            <span className="h-2 w-2 rounded-full" style={{ background: t.accent }} />
            <span className="h-1.5 w-12 rounded-full" style={{ background: t.text }} />
          </div>
          <span className="h-1.5 w-16 rounded-full" style={{ background: t.text3 }} />
          <div className="mt-auto flex items-center gap-1">
            <span className="rounded px-1.5 text-[9px] font-semibold leading-[14px]" style={{ background: t.accent, color: t.onAccent }}>
              Aa
            </span>
            <span className="h-1.5 w-1.5 rounded-full" style={{ background: t.amberMark }} />
            <span className="h-1.5 w-1.5 rounded-full" style={{ background: t.greenMark }} />
          </div>
        </div>
      </div>
      <div className="mt-1.5 flex items-center gap-1 px-0.5">
        <span className="flex-1 truncate text-[12.5px] font-medium">{p.name}</span>
        {selected && <Check size={13} className="shrink-0 text-accent" />}
      </div>
      <div className="truncate px-0.5 text-[11px] text-ink-3">
        {p.mood}
        {isDefault ? " · default" : ""}
      </div>
    </button>
  );
}
