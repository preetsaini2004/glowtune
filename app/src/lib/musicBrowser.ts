export type ServiceId = "spotify" | "youtube-music" | "apple-music" | "soundcloud";
export type MusicItem = { service: ServiceId; kind: string; title: string; artist: string; artwork: string | null; url: string; section?: string | null };
export type Shelf = { service: ServiceId; name: string; status: string; items: MusicItem[]; updatedAt: number };
/** `label`: what the list is when it isn't the player's own queue (SoundCloud autoplay). */
export type Queue = { items: MusicItem[]; message: string; label?: string };
export const SERVICE_NAMES: Record<ServiceId, string> = { spotify: "Spotify", "youtube-music": "YouTube Music", "apple-music": "Apple Music", soundcloud: "SoundCloud" };
