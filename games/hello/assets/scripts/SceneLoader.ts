// Several scenes at once (Unity's SceneManager): this object survives scene changes
// (dontDestroyOnLoad), loads another scene next to the current one (Additive) and reports what
// is loaded. Press A (K / Space) to switch the additive scene on / off.
import { Behaviour, SceneManager, LoadSceneMode, Input, Debug } from "goat";

export default class SceneLoader extends Behaviour {
  additive = "probes";
  private loaded = false;
  private busy = false;

  async start(): Promise<void> {
    this.dontDestroyOnLoad();
    SceneManager.sceneLoaded.addListener((s: any) => Debug.log("scene loaded:", s.name));
    SceneManager.sceneUnloaded.addListener((s: any) => Debug.log("scene unloaded:", s.name));
    await this.toggle();
  }

  update(): void {
    if (Input.getButtonDown("A") && !this.busy) void this.toggle();
  }

  private async toggle(): Promise<void> {
    this.busy = true;
    if (this.loaded) await SceneManager.unloadSceneAsync(this.additive);
    else await SceneManager.loadSceneAsync(this.additive, LoadSceneMode.Additive);
    this.loaded = !this.loaded;
    const names: string[] = [];
    for (let i = 0; i < SceneManager.sceneCount; i++) names.push(SceneManager.getSceneAt(i).name);
    Debug.log(`loaded scenes: ${names.join(", ")} (active: ${SceneManager.getActiveScene().name})`);
    this.busy = false;
  }
}
