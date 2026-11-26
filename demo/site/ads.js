// fake ad script — the blocklist should stop this from ever being fetched
window.track = function () { console.log("tracked"); };
