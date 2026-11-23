"use strict";

const motion = document.querySelector("#motion");
const deckArt = document.querySelector(".deck-sheet svg");
const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

const genreCycle = document.querySelector(".genre-cycle");
const genres = [...genreCycle.children];
const genreDuration = 2.6;
genreCycle.style.setProperty("--cycle-duration", `${genreDuration * genres.length}s`);
genres.forEach((genre, index) => {
  genre.style.animationDelay = `${index * genreDuration}s`;
});
genreCycle.classList.add("animated");

function setPaused(paused) {
  genreCycle.classList.toggle("paused", paused);
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
