/* tslint:disable */
/* eslint-disable */

export class EditorEngine {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    backend_name(): string;
    /**
     * JSON array of the registered component names.
     */
    component_names(): string;
    /**
     * JSON `[{ name, value }]` of an entity's components.
     */
    components(id: string): string;
    /**
     * Starts the engine on a canvas (WebGPU, or WebGL2 when unavailable).
     */
    static create(canvas: HTMLCanvasElement): Promise<EditorEngine>;
    create_entity(name: string, parent?: string | null): string;
    delete_entity(id: string): void;
    /**
     * One frame (`dt` seconds).
     */
    frame(dt: number): void;
    frame_selected(): void;
    /**
     * Starts a gizmo drag if (x, y) (0..1 of a w × h view) is on a handle.
     */
    gizmo_begin(x: number, y: number, w: number, h: number): boolean;
    gizmo_drag(dx: number, dy: number, w: number, h: number): void;
    gizmo_end(): void;
    /**
     * JSON array of `{ id, name, parent, kind }`.
     */
    hierarchy(): string;
    /**
     * Drops a model / prefab at a view point (0..1); returns the new entity.
     */
    instantiate(key: string, x: number, y: number): string;
    is_playing(): boolean;
    key(code: string, down: boolean): void;
    open_scene(key: string): void;
    orbit(dx: number, dy: number): void;
    pan(dx: number, dy: number): void;
    /**
     * Picks at (x, y) in 0..1 of the view; selects and returns the entity id.
     */
    pick(x: number, y: number): string | undefined;
    /**
     * Play mode; `dual_3ds` shows the game as the 3DS's two screens.
     */
    play(dual_3ds: boolean): void;
    /**
     * Makes a project file available to the engine (the UI reads files through Tauri).
     */
    put_file(key: string, bytes: Uint8Array | number[]): void;
    remove_component(id: string, name: string): void;
    rename(id: string, name: string): void;
    resize(width: number, height: number): void;
    save_scene(): string;
    scene(): string | undefined;
    select(id?: string | null): void;
    selected(): string | undefined;
    set_component(id: string, name: string, json: string): void;
    /**
     * "move" / "rotate" / "scale".
     */
    set_gizmo_mode(mode: string): void;
    set_parent(id: string, parent?: string | null): void;
    set_transform(id: string, json: string): void;
    /**
     * Draw calls / triangles of the last frame.
     */
    stats(): string;
    stop(): void;
    /**
     * `{ position, euler, scale }` of an entity.
     */
    transform(id: string): string | undefined;
    zoom(delta: number): void;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_editorengine_free: (a: number, b: number) => void;
    readonly editorengine_backend_name: (a: number) => [number, number];
    readonly editorengine_component_names: (a: number) => [number, number];
    readonly editorengine_components: (a: number, b: number, c: number) => [number, number];
    readonly editorengine_create: (a: any) => any;
    readonly editorengine_create_entity: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly editorengine_delete_entity: (a: number, b: number, c: number) => void;
    readonly editorengine_frame: (a: number, b: number) => void;
    readonly editorengine_frame_selected: (a: number) => void;
    readonly editorengine_gizmo_begin: (a: number, b: number, c: number, d: number, e: number) => number;
    readonly editorengine_gizmo_drag: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly editorengine_gizmo_end: (a: number) => void;
    readonly editorengine_hierarchy: (a: number) => [number, number];
    readonly editorengine_instantiate: (a: number, b: number, c: number, d: number, e: number) => [number, number, number, number];
    readonly editorengine_is_playing: (a: number) => number;
    readonly editorengine_key: (a: number, b: number, c: number, d: number) => void;
    readonly editorengine_open_scene: (a: number, b: number, c: number) => [number, number];
    readonly editorengine_orbit: (a: number, b: number, c: number) => void;
    readonly editorengine_pan: (a: number, b: number, c: number) => void;
    readonly editorengine_pick: (a: number, b: number, c: number) => [number, number];
    readonly editorengine_play: (a: number, b: number) => void;
    readonly editorengine_put_file: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly editorengine_remove_component: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly editorengine_rename: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly editorengine_resize: (a: number, b: number, c: number) => void;
    readonly editorengine_save_scene: (a: number) => [number, number, number, number];
    readonly editorengine_scene: (a: number) => [number, number];
    readonly editorengine_select: (a: number, b: number, c: number) => void;
    readonly editorengine_selected: (a: number) => [number, number];
    readonly editorengine_set_component: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => [number, number];
    readonly editorengine_set_gizmo_mode: (a: number, b: number, c: number) => void;
    readonly editorengine_set_parent: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly editorengine_set_transform: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly editorengine_stats: (a: number) => [number, number];
    readonly editorengine_stop: (a: number) => void;
    readonly editorengine_transform: (a: number, b: number, c: number) => [number, number];
    readonly editorengine_zoom: (a: number, b: number) => void;
    readonly wasm_bindgen_5ad06e3504d95cf8___convert__closures_____invoke___js_sys_ecc67c0883bd7d2___Function_fn_wasm_bindgen_5ad06e3504d95cf8___JsValue_____wasm_bindgen_5ad06e3504d95cf8___sys__Undefined___js_sys_ecc67c0883bd7d2___Function_fn_wasm_bindgen_5ad06e3504d95cf8___JsValue_____wasm_bindgen_5ad06e3504d95cf8___sys__Undefined_______true_: (a: number, b: number, c: any, d: any) => void;
    readonly wasm_bindgen_5ad06e3504d95cf8___convert__closures_____invoke___wasm_bindgen_5ad06e3504d95cf8___JsValue__core_9b3796e30d99ddb7___result__Result_____wasm_bindgen_5ad06e3504d95cf8___JsError___true_: (a: number, b: number, c: any) => [number, number];
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_destroy_closure: (a: number, b: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
