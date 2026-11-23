"use strict";

const motion = document.querySelector("#motion");
const deckArt = document.querySelector(".deck-sheet svg");
const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

function setPaused(paused) {
  deckArt.classList.toggle("paused", paused);
  motion.textContent = paused ? "Play animation" : "Pause animation";
}

function applyMotionPreference() {
  setPaused(reducedMotion.matches);
  motion.hidden = reducedMotion.matches;
}

motion.addEventListener("click", () => setPaused(!deckArt.classList.contains("paused")));
reducedMotion.addEventListener("change", applyMotionPreference);
applyMotionPreference();
