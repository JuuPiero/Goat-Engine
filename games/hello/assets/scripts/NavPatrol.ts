// Walks its NavMeshAgent to random points of the navmesh, one after another
// (scenes/navigation.scene.json). NavMesh.samplePosition finds a reachable point near a random
// spot; the agent arrived when its path is done and remainingDistance is under stoppingDistance.
import { Behaviour, NavMesh, NavMeshAgent, Time, Vector3, Gizmos, Handles, Color } from "goat";

export default class NavPatrol extends Behaviour {
  /** Half size of the square random points are picked in. */
  area = 8;
  /** Seconds to wait at each point. */
  wait = 0.5;

  private agent: NavMeshAgent = null as unknown as NavMeshAgent;
  private waiting = 0;
  private seed = 1;

  start(): void {
    this.agent = NavMeshAgent.of(this.gameObject);
    this.seed = (this.transform.position.x * 73 + this.transform.position.z * 151) | 0;
    this.pickNext();
  }

  update(): void {
    if (this.agent.pathPending || this.agent.remainingDistance > this.agent.stoppingDistance + 0.05) return;
    this.waiting += Time.deltaTime;
    if (this.waiting >= this.wait) this.pickNext();
  }

  private random(): number {
    // a small deterministic generator per agent (same runs on desktop and 3DS)
    this.seed = (this.seed * 1103515245 + 12345) & 0x7fffffff;
    return this.seed / 0x7fffffff;
  }

  /** Scene view (Edit and Play): the square the points are picked in. */
  onDrawGizmos(): void {
    Gizmos.color = new Color(0.3, 0.8, 1, 0.5);
    Gizmos.drawWireCube(new Vector3(0, 0.05, 0), new Vector3(this.area * 2, 0, this.area * 2));
  }

  /** Selected: where it is heading (while playing) and its settings. */
  onDrawGizmosSelected(): void {
    const p = this.transform.position;
    Handles.color = Color.yellow;
    Handles.label(p.add(new Vector3(0, 2.2, 0)), `${this.name}  area ${this.area}  wait ${this.wait}s`);
    Handles.drawWireDisc(p, Vector3.up, 0.8);
    const target = this.agent ? this.agent.destination : null;
    if (target) {
      Gizmos.color = Color.yellow;
      Gizmos.drawLine(p, target);
      Gizmos.drawWireSphere(target, 0.3);
    }
  }

  private pickNext(): void {
    this.waiting = 0;
    const p = new Vector3((this.random() * 2 - 1) * this.area, 0, (this.random() * 2 - 1) * this.area);
    const hit = NavMesh.samplePosition(p, 3);
    if (hit.hit && hit.position) this.agent.setDestination(hit.position);
  }
}
