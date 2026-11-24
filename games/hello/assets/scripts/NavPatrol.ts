// Walks its NavMeshAgent to random points of the navmesh, one after another
// (scenes/navigation.scene.json). NavMesh.samplePosition finds a reachable point near a random
// spot; the agent arrived when its path is done and remainingDistance is under stoppingDistance.
import { Behaviour, NavMesh, NavMeshAgent, Time, Vector3 } from "goat";

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

  private pickNext(): void {
    this.waiting = 0;
    const p = new Vector3((this.random() * 2 - 1) * this.area, 0, (this.random() * 2 - 1) * this.area);
    const hit = NavMesh.samplePosition(p, 3);
    if (hit.hit && hit.position) this.agent.setDestination(hit.position);
  }
}
