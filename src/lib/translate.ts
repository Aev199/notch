import { api, direction, type Provider, type Translation } from "./api";
import { translateOffline } from "./offline";

export async function translate(text: string, provider: Provider): Promise<Translation> {
  if (provider !== "offline") return api.translateOnline(text);
  const [from, to] = direction(text);
  return { text: await translateOffline(text, from, to), from, to, provider };
}
