// A small burst, for the one moment in a budgeting app that is plainly good
// news: a goal reached. It fires then and at no other time; confetti for
// ordinary events would wear out long before the goal did. Hand-rolled from
// mels-budget-app: one canvas, no dependency.

/** Read from the theme so the burst matches light and dark. */
const COLOURS = ["--brand", "--pos", "--violet", "--warn", "--brand-strong"];

// The feel of the burst. Distances are CSS pixels; speeds are per animation
// frame (about 1/60 s), so these are tuned by eye, not physics.
const BURST_ORIGINS = [0.15, 0.85]; // across the screen, as a fraction of its width
const LAUNCH_HEIGHT = 0.92; // down the screen, as a fraction of its height
const PIECES_PER_BURST = 60;
const SPREAD = 1.1; // radians of scatter around straight up
const LEAN_INWARD = 0.35; // radians each burst tilts toward the middle
const MIN_SPEED = 9;
const EXTRA_SPEED = 9; // random, on top of MIN_SPEED
const GRAVITY = 0.28; // added to downward speed each frame
const AIR_DRAG = 0.995; // sideways speed kept each frame
const MAX_SPIN = 0.3; // radians per frame, either way
const LIFE_MS = 2600;
const FADE_CURVE = 0.6; // below 1, pieces stay bright longer before fading
/** Extra time before the canvas is removed even if no frame ever ran. */
const SWEEP_GRACE_MS = 2000;

/** One scrap of paper: position and speed in CSS pixels, angles in radians. */
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

/** Fire the burst once. Safe to call anywhere; it cleans up after itself. */
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
  for (const origin of BURST_ORIGINS) {
    for (let i = 0; i < PIECES_PER_BURST; i++) {
      // Straight up is -π/2 on a canvas, whose y grows downward.
      const angle = -Math.PI / 2 + (Math.random() - 0.5) * SPREAD + (origin < 0.5 ? LEAN_INWARD : -LEAN_INWARD);
      const speed = MIN_SPEED + Math.random() * EXTRA_SPEED;
      pieces.push({
        x: width * origin,
        y: height * LAUNCH_HEIGHT,
        vx: Math.cos(angle) * speed,
        vy: Math.sin(angle) * speed,
        rotation: Math.random() * Math.PI,
        spin: (Math.random() - 0.5) * MAX_SPIN,
        // Small paper rectangles, 6–11 px by 3–7 px.
        width: 6 + Math.random() * 5,
        height: 3 + Math.random() * 4,
        // Grey if the theme lacks the variable.
        colour: styles.getPropertyValue(COLOURS[i % COLOURS.length]).trim() || "#888",
      });
    }
  }

  const started = performance.now();
  // requestAnimationFrame doesn't run in a hidden window; sweep up regardless.
  const sweepUp = window.setTimeout(() => canvas.remove(), LIFE_MS + SWEEP_GRACE_MS);

  function frame(now: number) {
    const elapsed = now - started;
    if (elapsed > LIFE_MS) {
      window.clearTimeout(sweepUp);
      canvas.remove();
      return;
    }
    context!.clearRect(0, 0, width, height);
    context!.globalAlpha = Math.max(0, 1 - elapsed / LIFE_MS) ** FADE_CURVE;
    for (const p of pieces) {
      p.vy += GRAVITY;
      p.vx *= AIR_DRAG;
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
