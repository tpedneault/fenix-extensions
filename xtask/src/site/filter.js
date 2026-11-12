// The list's search and category filters. Without this script the list
// is all there, unfiltered; with it, typing narrows the cards.
(function () {
  "use strict";
  var form = document.getElementById("filters");
  if (!form) return;
  var q = document.getElementById("q");
  var shown = document.getElementById("shown");
  var none = document.getElementById("none");
  var cards = Array.prototype.slice.call(document.querySelectorAll(".card"));
  var groups = Array.prototype.slice.call(document.querySelectorAll(".group"));
  var total = cards.length;

  function category() {
    var on = form.querySelector("input[name=category]:checked");
    return on ? on.value : "";
  }

  function apply() {
    var words = q.value.toLowerCase().trim().split(/\s+/).filter(Boolean);
    var cat = category();
    var count = 0;
    cards.forEach(function (card) {
      var text = card.getAttribute("data-search");
      var cats = card.getAttribute("data-categories").split(" ");
      var match = (!cat || cats.indexOf(cat) >= 0) && words.every(function (w) { return text.indexOf(w) >= 0; });
      card.hidden = !match;
      if (match) count++;
    });
    groups.forEach(function (group) {
      group.hidden = !group.querySelector(".card:not([hidden])");
    });
    none.hidden = count > 0;
    shown.textContent = count === total ? "Showing all " + total : "Showing " + count + " of " + total;
  }

  // ?q=…&category=… from a link (an extension's page links its categories here).
  var params = new URLSearchParams(location.search);
  if (params.get("q")) q.value = params.get("q");
  var wanted = params.get("category");
  if (wanted) {
    var radio = document.getElementById("cat-" + wanted);
    if (radio && radio.name === "category") radio.checked = true;
  }

  form.hidden = false;
  form.addEventListener("submit", function (e) { e.preventDefault(); });
  q.addEventListener("input", apply);
  form.addEventListener("change", apply);
  // "/" to search, as in many sites; not while typing somewhere.
  document.addEventListener("keydown", function (e) {
    var t = e.target.tagName;
    if (e.key === "/" && t !== "INPUT" && t !== "TEXTAREA" && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      q.focus();
    }
  });
  apply();
})();
