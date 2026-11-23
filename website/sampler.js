"use strict";

const sampleBank = document.querySelector(".sample-bank");
const sampleStatus = document.querySelector(".sample-status");

for (const pad of sampleBank.querySelectorAll("button")) {
  const sample = new Audio();
  sample.preload = "none";
  sample.src = pad.dataset.audio;
  sample.addEventListener("playing", () => pad.classList.add("sounding"));
  sample.addEventListener("ended", () => pad.classList.remove("sounding"));
  pad.addEventListener("click", () => {
    sampleStatus.textContent = "";
    sample.currentTime = 0;
    sample.play().catch(() => {
      pad.classList.remove("sounding");
      sampleStatus.textContent = "This preview could not play. Try again.";
    });
  });
}
sampleBank.hidden = false;
