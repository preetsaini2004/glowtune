import { Composition, Still } from "remotion";
import { Clip, Hero, Poster } from "./Composition";
import {
  CLIP_SECONDS,
  FPS,
  HERO_SECONDS,
  features,
  type Feature,
} from "./edit";

export const RemotionRoot = () => (
  <>
    <Composition
      id="GlowtuneHero"
      component={Hero}
      defaultProps={{ repository: "" }}
      width={1920}
      height={1080}
      fps={FPS}
      durationInFrames={HERO_SECONDS * FPS}
    />
    <Still id="GlowtunePoster" component={Poster} width={1920} height={1080} />
    <Composition
      id="Glowtune02Hero"
      component={Hero}
      defaultProps={{
        repository: "github.com/preetsaini2004/glowtune",
        production: true,
      }}
      width={1920}
      height={1080}
      fps={FPS}
      durationInFrames={75 * FPS}
    />
    <Still
      id="Glowtune02Poster"
      component={Poster}
      defaultProps={{ production: true }}
      width={1920}
      height={1080}
    />
    {(Object.keys(features) as Feature[]).map((feature) => (
      <Composition
        key={feature}
        id={feature}
        component={Clip}
        defaultProps={{ feature }}
        width={1280}
        height={720}
        fps={FPS}
        durationInFrames={CLIP_SECONDS * FPS}
      />
    ))}
  </>
);
