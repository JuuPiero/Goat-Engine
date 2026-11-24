// Slides its object up and down every `period` seconds (a door with a carving NavMeshObstacle:
// closed, agents plan around through the other gap; open, they use it again).
import { Behaviour, Time, Vector3 } from "goat";

export default class SlidingDoor extends Behaviour {
  period = 4;
  lift = 2.2;

  private closed = Vector3.zero;
  private t = 0;

  start(): void {
    this.closed = this.transform.position;
  }

  update(): void {
    this.t += Time.deltaTime;
    const open = Math.floor(this.t / this.period) % 2 === 1;
    const target = this.closed.add(new Vector3(0, open ? this.lift : 0, 0));
    const p = this.transform.position;
    const step = 3 * Time.deltaTime;
    const dy = Math.max(-step, Math.min(step, target.y - p.y));
    this.transform.position = new Vector3(p.x, p.y + dy, p.z);
  }
}
