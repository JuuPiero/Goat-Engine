// Low-level graphics like Unity's: a swarm of cubes drawn with Graphics.drawMeshInstanced (a color per
// instance, no GameObjects), a debug ray, a GL overlay in screen space, and a Texture2D made in code.
import { Behaviour, Graphics, GL, Debug, Color, Vector3, Quaternion, Matrix4x4, MaterialPropertyBlock, Texture2D, Renderer, GameObject, Time } from "goat";

export default class GraphicsDemo extends Behaviour {
  count = 120;
  radius = 3.2;
  private block = new MaterialPropertyBlock();
  private colors: number[] = [];

  start(): void {
    for (let i = 0; i < this.count; i++) {
      const c = Color.HSVToRGB(i / this.count, 0.7, 1);
      this.colors.push(c.r, c.g, c.b, 1);
    }
    this.block.setVectorArray("_Color", this.colors);
    // a checker made pixel by pixel, shown on the crate
    const tex = new Texture2D(8, 8);
    tex.filterMode = "point";
    for (let y = 0; y < 8; y++) for (let x = 0; x < 8; x++) tex.setPixel(x, y, (x + y) % 2 === 0 ? new Color(1, 0.8, 0.2) : new Color(0.15, 0.2, 0.4));
    tex.apply();
    const crate = GameObject.find("Crate");
    if (crate) Renderer.of(crate).material.mainTexture = tex;
  }

  update(): void {
    const t = Time.time;
    const matrices: Matrix4x4[] = [];
    for (let i = 0; i < this.count; i++) {
      const a = (i / this.count) * Math.PI * 2 + t * 0.4;
      const y = 1.6 + Math.sin(a * 3 + t * 2) * 0.35;
      const p = new Vector3(Math.cos(a) * this.radius, y, Math.sin(a) * this.radius * 0.6 - 0.4);
      matrices.push(Matrix4x4.TRS(p, Quaternion.euler(t * 60 + i * 7, i * 13, 0), new Vector3(0.18, 0.18, 0.18)));
    }
    Graphics.drawMeshInstanced("Cube", 0, "builtin:default", matrices, this.count, this.block);
    Debug.drawRay(new Vector3(0, 0.05, 0.5), new Vector3(Math.cos(t), 0, Math.sin(t)).mul(2.5), Color.yellow);
    // a frame around the screen + a diagonal, in GL's 0..1 screen space
    GL.pushMatrix();
    GL.loadOrtho();
    GL.begin(GL.LINE_STRIP);
    GL.color(new Color(0.4, 1, 0.6, 1));
    GL.vertex3(0.02, 0.03, 0); GL.vertex3(0.98, 0.03, 0); GL.vertex3(0.98, 0.97, 0); GL.vertex3(0.02, 0.97, 0); GL.vertex3(0.02, 0.03, 0);
    GL.end();
    GL.begin(GL.TRIANGLES);
    GL.color(new Color(1, 0.3, 0.3, 0.6));
    GL.vertex3(0.04, 0.06, 0); GL.vertex3(0.12, 0.06, 0); GL.vertex3(0.04, 0.16, 0);
    GL.end();
    GL.popMatrix();
  }
}
