// Run directly after the header so the entrance can use the page's own logo.
(() => {
  const motion = matchMedia("(prefers-reduced-motion: reduce)");
  if (motion.matches || location.hash) return;

  const seenKey = "dubplate-groove-intro";
  try {
    if (sessionStorage.getItem(seenKey)) return;
    sessionStorage.setItem(seenKey, "seen");
  } catch {
    // Storage can be unavailable when opening the site directly from disk.
    // Skip the intro in that case rather than repeat it on every navigation.
    return;
  }

  const mark = document.querySelector(".site-header .logo svg").cloneNode(true);
  const guide = mark.querySelector("path");
  guide.setAttribute("stroke", "currentColor");
  const trace = guide.cloneNode(true);
  guide.setAttribute("opacity", ".18");
  trace.setAttribute("pathLength", "1");
  trace.setAttribute("stroke-dasharray", "1 1");
  mark.append(trace);

  const entrance = document.createElement("div");
  entrance.className = "groove-entrance";
  entrance.setAttribute("aria-hidden", "true");
  const name = document.createElement("span");
  name.textContent = "dubplate";
  entrance.append(mark, name);
  document.body.prepend(entrance);

  const events = new AbortController();
  function finish() {
    entrance.remove();
    events.abort();
    clearTimeout(fallback);
  }

  const fallback = setTimeout(finish, 3500);
  entrance.addEventListener("pointerdown", finish, { signal: events.signal });
  window.addEventListener("keydown", finish, { signal: events.signal });
  window.addEventListener("pagehide", finish, { signal: events.signal });
  motion.addEventListener("change", finish, { signal: events.signal });

  // Draw toward the centre for 1.6s, hold for 0.5s, then erase from the rim.
  const ease = "cubic-bezier(.37,0,.63,1)";
  trace.animate([
    { strokeDashoffset: "1", offset: 0, easing: ease },
    { strokeDashoffset: "0", offset: 1.6 / 3.3 },
    { strokeDashoffset: "0", offset: 2.1 / 3.3, easing: ease },
    { strokeDashoffset: "-1", offset: 1 },
  ], { duration: 3300, fill: "both" });

  const reveal = entrance.animate([
    { opacity: 1, offset: 0 },
    { opacity: 1, offset: .84, easing: "ease-out" },
    { opacity: 0, offset: 1 },
  ], { duration: 3300, fill: "both" });
  reveal.onfinish = finish;
})();
