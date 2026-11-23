"use strict";

const genreCycle = document.querySelector(".genre-cycle");
const genres = [...genreCycle.children];
const genreDuration = 2.6;
genreCycle.style.setProperty("--cycle-duration", `${genreDuration * genres.length}s`);
genres.forEach((genre, index) => {
  genre.style.animationDelay = `${index * genreDuration}s`;
});
genreCycle.classList.add("animated");
