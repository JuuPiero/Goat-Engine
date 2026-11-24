// A TypeScript behaviour: floats its object up and down and turns it slowly.
// Attached in scenes/showcase.scene.json: "Script": { "src": "scripts/Hover.ts", "props": { ... } }
import { Behaviour, Mathf, Time, Vector3 } from "goat";

export default class Hover extends Behaviour {
  /** Units up and down from the start position. */
  height = 0.25;
  /** Cycles per second. */
  speed = 0.5;
  /** Degrees per second around Y. */
  turn = 30;

  private base = Vector3.zero;
  private t = 0;

  start(): void {
    this.base = this.transform.position;
  }

  update(): void {
    this.t += Time.deltaTime;
    const y = Math.sin(this.t * this.speed * 2 * Mathf.PI) * this.height;
    this.transform.position = this.base.add(new Vector3(0, y, 0));
    this.transform.rotate(0, this.turn * Time.deltaTime, 0);
  }
}
