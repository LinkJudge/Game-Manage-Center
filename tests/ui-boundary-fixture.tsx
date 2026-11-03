import { createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { ErrorBoundary } from "@/components/errorBoundary";
let fails = true;
let root: Root;
function Fixture() {
  if (fails) throw new Error("gmc-boundary-fixture");
  return createElement("span", null, "fixture recovered");
}
export function mount() {
  const box = document.createElement("div");
  box.id = "boundary-fixture";
  document.body.appendChild(box);
  fails = true;
  root = createRoot(box);
  root.render(createElement(ErrorBoundary, null, createElement(Fixture)));
}
export function recover() { fails = false; }
export function unmount() { root.unmount(); document.getElementById("boundary-fixture")?.remove(); }
