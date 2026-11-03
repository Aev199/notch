// Offline RU⇄EN translation: Bergamot (Firefox Translations engine) compiled
// to WebAssembly, running in a worker inside the webview. Models are embedded
// in the exe and fetched over IPC as gzip; nothing touches the network.
import { invoke } from "@tauri-apps/api/core";
import type { Lang } from "./api";

type Pair = "ruen" | "enru";
const PARTS = ["model", "lex", "vocab"] as const;

interface Translator {
  translate(req: { from: string; to: string; text: string; html: boolean }): Promise<{ target: { text: string } }>;
}

let translator: Promise<Translator> | null = null;

async function loadPart(pair: Pair, part: string): Promise<ArrayBuffer> {
  const gz = await invoke<ArrayBuffer>("offline_model", { pair, part });
  const stream = new Blob([gz]).stream().pipeThrough(new DecompressionStream("gzip"));
  return new Response(stream).arrayBuffer();
}

async function create(): Promise<Translator> {
  // Runtime import of the static copy in /bergamot (see scripts/copy-bergamot.mjs).
  const url = "/bergamot/translator.js";
  const mod = await import(/* @vite-ignore */ url);
  const Backing = mod.TranslatorBacking as new (options: object) => object;
  class EmbeddedBacking extends Backing {
    async loadModelRegistery() {
      return (["ruen", "enru"] as Pair[]).map((pair) => ({
        from: pair.slice(0, 2),
        to: pair.slice(2),
        files: Object.fromEntries(PARTS.map((part) => [part, { name: `${pair}/${part}` }])),
      }));
    }
    async fetch(name: string) {
      const [pair, part] = name.split("/");
      return loadPart(pair as Pair, part);
    }
  }
  const options = { pivotLanguage: null, cacheSize: 256 };
  return new mod.LatencyOptimisedTranslator(options, new EmbeddedBacking(options));
}

/** Loads the engine in the background so the first translation is quick. */
export function warmUp() {
  translator ??= create();
  translator.catch(() => (translator = null));
}

export async function translateOffline(text: string, from: Lang, to: Lang): Promise<string> {
  translator ??= create();
  let engine: Translator;
  try {
    engine = await translator;
  } catch (e) {
    translator = null;
    throw e;
  }
  const result = await engine.translate({ from, to, text, html: false });
  return result.target.text;
}

/** A newer request replaced this one: not an error worth showing. */
export function isSuperseded(e: unknown): boolean {
  return e instanceof Error && (e.name === "SupersededError" || e.constructor?.name === "SupersededError");
}
