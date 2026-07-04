import "./style.css";
import { createClient } from "~bridge/client";

const baseUrl = "http://127.0.0.1:8787";
const client = createClient(baseUrl);

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) {
  throw new Error("missing #app");
}

app.innerHTML = `
  <h1>Bridge Framework Vite Frontend</h1>
  <p>Backend base URL: <code>${baseUrl}</code></p>
  <div>
    <button id="health">Health</button>
    <button id="modeGet">Get Mode</button>
    <input id="modeValue" value="full" />
    <button id="modeSet">Set Mode</button>
  </div>
  <h2>Compiler Input</h2>
  <textarea id="source">service hello
endpoint ping GET /ping
endpoint echo POST /echo</textarea>
  <div>
    <button id="compile">Compile + Codegen</button>
    <button id="latest">Load Latest</button>
  </div>
  <pre id="output"></pre>
`;

const output = document.querySelector<HTMLPreElement>("#output");
const source = document.querySelector<HTMLTextAreaElement>("#source");
const modeValue = document.querySelector<HTMLInputElement>("#modeValue");
if (!output || !source || !modeValue) {
  throw new Error("missing UI elements");
}

function show(value: string) {
  output.textContent = value;
}

document.querySelector<HTMLButtonElement>("#health")!.onclick = async () => {
  show(await client.health());
};

document.querySelector<HTMLButtonElement>("#modeGet")!.onclick = async () => {
  show(await client.modeGet());
};

document.querySelector<HTMLButtonElement>("#modeSet")!.onclick = async () => {
  show(await client.modeSet(modeValue.value));
};

document.querySelector<HTMLButtonElement>("#compile")!.onclick = async () => {
  show(await client.compile(source.value));
};

document.querySelector<HTMLButtonElement>("#latest")!.onclick = async () => {
  show(await client.latest());
};
