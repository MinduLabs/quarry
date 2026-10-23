/** The startup splash from index.html: fades out once the app has painted, then goes away. */
export function hideSplash() {
  const splash = document.getElementById("splash");
  if (!splash || splash.classList.contains("is-done")) return;
  // After the frame that shows the app, so the splash never uncovers an empty window.
  requestAnimationFrame(() => {
    splash.classList.add("is-done");
    splash.addEventListener("transitionend", () => splash.remove(), { once: true });
    // Reduced motion or a hidden window may skip the transition.
    setTimeout(() => splash.remove(), 1_000);
  });
}
