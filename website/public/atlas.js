"use strict";

const tracks = [
  {
    x: 190,
    title: "Open the floor.",
    note: "Start with space. Mark the mix-out cue on the opener so you can rehearse the handoff in headphones.",
  },
  {
    x: 625,
    title: "Bring the peak in.",
    note: "A six-BPM lift changes the feel of the handoff. Preview the transition and decide where to bring in the incoming bass.",
  },
  {
    x: 930,
    title: "Leave some air.",
    note: "The closing pair shares a key. Set the mix-in cue after the incoming vocal and listen to the overlap.",
  },
];

const mapWidth = 1080;
const mapHeight = 230;
const contours = document.querySelector("#contours");
const marker = document.querySelector("#marker");
const layout = document.querySelector(".map-layout");
const controls = document.querySelector(".tracks");
const buttons = [...controls.querySelectorAll("button")];
const noteTitle = document.querySelector("#atlas-note-title");
const noteText = document.querySelector("#atlas-note-text");

function energyHeight(x) {
  const peak = 84 * Math.exp(-((x - 623) ** 2) / 29500);
  return 151 - peak + 13 * Math.sin(x / 150) + 10 * Math.sin(x / 82);
}

function drawContours() {
  const paths = document.createDocumentFragment();
  for (let line = -12; line <= 18; line++) {
    const points = [];
    for (let x = 0; x <= mapWidth; x += 8) {
      const spacing = line * (4 + .8 * Math.sin(x / 123));
      const texture = line ** 2 * .067 * Math.sin((x + line * 3) / 78);
      points.push(`${x},${(energyHeight(x) + spacing + texture).toFixed(1)}`);
    }
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("d", `M${points.join("L")}`);
    path.setAttribute("stroke", line === 0 ? "#edff99" : line % 4 === 0 ? "#b4d788" : "#5f9c81");
    path.setAttribute("stroke-width", line === 0 ? "3" : "1");
    path.setAttribute("vector-effect", "non-scaling-stroke");
    paths.append(path);
  }
  contours.replaceChildren(paths);
}

function showTrack(index) {
  const track = tracks[index];
  buttons.forEach((button, position) => {
    button.setAttribute("aria-pressed", String(position === index));
  });
  marker.setAttribute("transform", `translate(${track.x} 0)`);
  layout.style.setProperty("--marker-x", `${100 * track.x / mapWidth}%`);
  layout.style.setProperty("--marker-y", `${100 * energyHeight(track.x) / mapHeight}%`);
  noteTitle.textContent = track.title;
  noteText.textContent = track.note;
}

drawContours();
buttons.forEach((button, index) => {
  button.addEventListener("click", () => showTrack(index));
});
showTrack(0);
controls.hidden = false;
