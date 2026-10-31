const count = document.querySelector("#count");
let clicks = 0;

document.querySelector("#more").addEventListener("click", () => {
  clicks += 1;
  count.textContent = String(clicks);
});
