/* A behaviour in C: bobs its entity up and down, spawns a marker on A, reads / writes a
 * component as JSON, reports through an engine extension call. */
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "goat.h"

typedef struct {
    float amount;
    float t;
    float base_y;
    int spawned;
} Wobble;

static void* wobble_create(GoatApi* g, GoatEntity self, const char* props) {
    (void)g;
    (void)self;
    Wobble* w = (Wobble*)calloc(1, sizeof(Wobble));
    w->amount = 1.0f;
    /* tiny props parsing: {"amount": N} */
    const char* a = props ? strstr(props, "\"amount\"") : NULL;
    if (a) {
        a = strchr(a, ':');
        if (a) w->amount = (float)atof(a + 1);
    }
    return w;
}

static void wobble_start(void* state, GoatApi* g, GoatEntity self) {
    Wobble* w = (Wobble*)state;
    w->base_y = goat_get_position(g, self).y;
    char name[64];
    goat_get_name(g, self, name, (int)sizeof name);
    char msg[128];
    snprintf(msg, sizeof msg, "wobble start on %s (amount %.1f)", name, w->amount);
    goat_log(g, GOAT_LOG_INFO, msg);
}

static void wobble_update(void* state, GoatApi* g, GoatEntity self) {
    Wobble* w = (Wobble*)state;
    w->t += goat_delta_time(g);
    GoatVec3 p = goat_get_position(g, self);
    p.y = w->base_y + sinf(w->t * 3.14159265f) * w->amount;
    goat_set_position(g, self, p);

    if ((goat_button(g, "A") & GOAT_PRESSED) && !w->spawned) {
        GoatEntity m = goat_spawn(g, "Marker");
        GoatVec3 at = { 0.0f, 7.0f, 0.0f };
        goat_set_position(g, m, at);
        goat_set_component(g, m, "MeshRenderer", "{\"mesh\":\"builtin:sphere\"}");
        char json[256];
        if (goat_get_component(g, m, "MeshRenderer", json, (int)sizeof json) > 0 && strstr(json, "builtin:sphere")) {
            goat_set_name(g, m, "Marker:sphere");
        }
        char out[64];
        goat_call(g, "Test.double", self, "[21]", out, (int)sizeof out);
        if (strcmp(out, "42.0") == 0 || strcmp(out, "42") == 0) w->spawned = 1;
    }
}

static void wobble_destroy(void* state, GoatApi* g, GoatEntity self) {
    (void)state;
    (void)self;
    goat_log(g, GOAT_LOG_INFO, "wobble destroyed");
}

static void wobble_free(void* state) { free(state); }

static const GoatBehaviourClass WOBBLE = {
    "Wobble", wobble_create, wobble_start, wobble_update, NULL, NULL, wobble_destroy, wobble_free,
};

void goat_example_c_init(void) { goat_register_behaviour(&WOBBLE); }
