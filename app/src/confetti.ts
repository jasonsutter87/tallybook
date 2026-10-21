// A small burst, for the one moment in a budgeting app that is plainly good
// news: a goal reached. It fires then and at no other time; confetti for
// ordinary events would wear out long before the goal did. Hand-rolled from
// mels-budget-app: one canvas, no dependency.

/** Read from the theme so the burst matches light and dark. */
const COLOURS = ["--brand", "--pos", "--violet", "--warn", "--brand-strong"];

interface Piece {
  x: number;
  y: number;
  vx: number;
  vy: number;
  rotation: number;
  spin: number;
  width: number;
  height: number;
  colour: string;
}

export function celebrate(): void {
  if (typeof window === "undefined") return;
  // Someone who asked for less motion asked for this too.
  if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) return;

  const canvas = document.createElement("canvas");
  canvas.style.cssText = "position:fixed;inset:0;width:100%;height:100%;pointer-events:none;z-index:1000";
  canvas.setAttribute("aria-hidden", "true");
  document.body.appendChild(canvas);

  const scale = window.devicePixelRatio || 1;
  canvas.width = window.innerWidth * scale;
  canvas.height = window.innerHeight * scale;
  const context = canvas.getContext("2d");
  if (!context) {
    canvas.remove();
    return;
  }
  context.scale(scale, scale);

  const styles = getComputedStyle(document.documentElement);
  const width = window.innerWidth;
  const height = window.innerHeight;

  // Two bursts from the lower corners: celebration, not weather.
  const pieces: Piece[] = [];
  for (const origin of [0.15, 0.85]) {
    for (let i = 0; i < 60; i++) {
      const angle = -Math.PI / 2 + (Math.random() - 0.5) * 1.1 + (origin < 0.5 ? 0.35 : -0.35);
      const speed = 9 + Math.random() * 9;
      pieces.push({
        x: width * origin,
        y: height * 0.92,
        vx: Math.cos(angle) * speed,
        vy: Math.sin(angle) * speed,
        rotation: Math.random() * Math.PI,
        spin: (Math.random() - 0.5) * 0.3,
        width: 6 + Math.random() * 5,
        height: 3 + Math.random() * 4,
        colour: styles.getPropertyValue(COLOURS[i % COLOURS.length]).trim() || "#888",
      });
    }
  }

  const started = performance.now();
  const LIFE = 2600;
  // requestAnimationFrame doesn't run in a hidden window; sweep up regardless.
  const sweepUp = window.setTimeout(() => canvas.remove(), LIFE + 2000);

  function frame(now: number) {
    const elapsed = now - started;
    if (elapsed > LIFE) {
      window.clearTimeout(sweepUp);
      canvas.remove();
      return;
    }
    context!.clearRect(0, 0, width, height);
    context!.globalAlpha = Math.max(0, 1 - elapsed / LIFE) ** 0.6;
    for (const p of pieces) {
      p.vy += 0.28;
      p.vx *= 0.995;
      p.x += p.vx;
      p.y += p.vy;
      p.rotation += p.spin;
      context!.save();
      context!.translate(p.x, p.y);
      context!.rotate(p.rotation);
      context!.fillStyle = p.colour;
      context!.fillRect(-p.width / 2, -p.height / 2, p.width, p.height);
      context!.restore();
    }
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);
}
