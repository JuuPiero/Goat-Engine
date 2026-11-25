// Data files as fields and Unity's Resources, with Inspector decorators like PiaEngine / Odin:
// TextAsset / JsonAsset fields (drop files from the Project panel), Resources.loadAll of the
// enemies under assets/Resources/Enemies, @header / @range / @tooltip / @showIf, and @button.
import { Behaviour, Debug, TextAsset, JsonAsset, Resources, field, header, range, tooltip, showIf, button, hideInInspector } from "goat";

interface Level { name: string; enemies: string[]; time: number }
interface Enemy { hp: number; speed: number }

export default class LevelManager extends Behaviour {
  @header("Data")
  @field(TextAsset) credits: TextAsset | null = null;
  @field([JsonAsset]) levels: JsonAsset[] = [];

  @header("Rules")
  @tooltip("The level played first (index in Levels)")
  @range(0, 9) startLevel = 0;
  hardMode = false;
  @showIf("hardMode") @range(1, 5) enemyMultiplier = 2;

  @hideInInspector plays = 0;

  start(): void {
    Debug.log(`credits: ${this.credits?.lines.length ?? 0} lines — "${this.credits?.lines[0] ?? ""}"`);
    const level = this.levels[this.startLevel]?.json as Level | undefined;
    if (level) Debug.log(`level ${this.startLevel}: ${level.name}, ${level.enemies.length} enemies, ${level.time}s`);
    for (const e of Resources.loadAll("Enemies", JsonAsset) as JsonAsset[]) {
      const stats = e.json as Enemy;
      const hp = this.hardMode ? stats.hp * this.enemyMultiplier : stats.hp;
      Debug.log(`enemy ${e.name}: hp ${hp}, speed ${stats.speed}`);
    }
  }

  /** Inspector button: works in Edit mode too (the change is saved like a field edit). */
  @button("Next Level")
  nextLevel(): void {
    this.startLevel = this.levels.length ? (this.startLevel + 1) % this.levels.length : 0;
    this.plays++;
  }
}
