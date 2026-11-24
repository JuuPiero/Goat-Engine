/* GoatEngine — the C ABI for native scripts (C, C++, or any language that can call C).
 *
 * A native behaviour is a class of callbacks registered once at startup:
 *
 *     static void* wobble_create(GoatApi* g, GoatEntity self, const char* props_json) { ... }
 *     static void  wobble_update(void* state, GoatApi* g, GoatEntity self) { ... }
 *     static const GoatBehaviourClass WOBBLE = { "Wobble", wobble_create, NULL, wobble_update, ... };
 *     void goat_game_init(void) { goat_register_behaviour(&WOBBLE); }
 *
 * Scenes attach it like any script: "Script": { "src": "native:Wobble", "props": { "amount": 2 } }.
 * The GoatApi pointer is only valid during the callback that received it. Strings are UTF-8.
 * Functions writing text into a buffer return the full length (call again with a bigger buffer
 * when it is >= cap); -1 means "nothing" (no such component...).
 */
#ifndef GOAT_H
#define GOAT_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct GoatApi GoatApi;
/* An entity (GameObject) handle; GOAT_NONE = no entity. */
typedef uint32_t GoatEntity;
#define GOAT_NONE 0xFFFFFFFFu

typedef struct GoatVec2 { float x, y; } GoatVec2;
typedef struct GoatVec3 { float x, y, z; } GoatVec3;

enum { GOAT_LOG_INFO = 2, GOAT_LOG_WARN = 3, GOAT_LOG_ERROR = 4 };
enum { GOAT_HELD = 1, GOAT_PRESSED = 2, GOAT_RELEASED = 4 };

typedef struct GoatBehaviourClass {
    const char* name;
    /* Returns the instance's state (may be NULL). props_json: the scene's "props" ("null" if none). */
    void* (*create)(GoatApi* g, GoatEntity self, const char* props_json);
    void (*start)(void* state, GoatApi* g, GoatEntity self);
    void (*update)(void* state, GoatApi* g, GoatEntity self);
    void (*fixed_update)(void* state, GoatApi* g, GoatEntity self);
    void (*late_update)(void* state, GoatApi* g, GoatEntity self);
    void (*on_destroy)(void* state, GoatApi* g, GoatEntity self);
    /* Frees the state returned by create. */
    void (*free_state)(void* state);
} GoatBehaviourClass;

/* Registers a class (keep it alive: a static). Call before the game starts. */
void goat_register_behaviour(const GoatBehaviourClass* cls);

/* entities */
GoatEntity goat_find(GoatApi* g, const char* name);
GoatEntity goat_spawn(GoatApi* g, const char* name);
GoatEntity goat_instantiate(GoatApi* g, const char* asset_key, GoatEntity parent);
void goat_destroy(GoatApi* g, GoatEntity e);
int goat_exists(GoatApi* g, GoatEntity e);
int goat_get_name(GoatApi* g, GoatEntity e, char* buf, int cap);
void goat_set_name(GoatApi* g, GoatEntity e, const char* name);
GoatEntity goat_get_parent(GoatApi* g, GoatEntity e);
void goat_set_parent(GoatApi* g, GoatEntity e, GoatEntity parent);

/* transform (local; degrees) */
GoatVec3 goat_get_position(GoatApi* g, GoatEntity e);
void goat_set_position(GoatApi* g, GoatEntity e, GoatVec3 p);
GoatVec3 goat_get_euler(GoatApi* g, GoatEntity e);
void goat_set_euler(GoatApi* g, GoatEntity e, GoatVec3 degrees);
void goat_rotate(GoatApi* g, GoatEntity e, GoatVec3 degrees);
GoatVec3 goat_get_scale(GoatApi* g, GoatEntity e);
void goat_set_scale(GoatApi* g, GoatEntity e, GoatVec3 s);
GoatVec3 goat_get_world_position(GoatApi* g, GoatEntity e);

/* time, input */
float goat_delta_time(GoatApi* g);
float goat_time(GoatApi* g);
/* "A", "B", "X", "Y", "L", "R", "start", "dpad/up"... → GOAT_HELD | GOAT_PRESSED | GOAT_RELEASED bits */
int goat_button(GoatApi* g, const char* name);
GoatVec2 goat_stick(GoatApi* g);
GoatVec2 goat_action_value(GoatApi* g, const char* action);
int goat_action_triggered(GoatApi* g, const char* action);

/* log */
void goat_log(GoatApi* g, int level, const char* message);

/* components as their scene JSON */
int goat_get_component(GoatApi* g, GoatEntity e, const char* component, char* json, int cap);
/* 0 = done, otherwise an error (logged) */
int goat_set_component(GoatApi* g, GoatEntity e, const char* component, const char* json);
void goat_remove_component(GoatApi* g, GoatEntity e, const char* component);
/* An engine call added by a subsystem / the game ("Animator.setTrigger"); args: a JSON array. */
int goat_call(GoatApi* g, const char* name, GoatEntity e, const char* args_json, char* result_json, int cap);

#ifdef __cplusplus
}
#endif

#endif /* GOAT_H */
