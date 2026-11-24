// A behaviour in C++: a class wrapped by a small template that fills the C vtable.
#include <cmath>
#include <string>

#include "goat.h"

namespace goat {

// Behaviour base: override what you need.
struct Behaviour {
    virtual ~Behaviour() = default;
    virtual void start(GoatApi*, GoatEntity) {}
    virtual void update(GoatApi*, GoatEntity) {}
};

template <class T>
GoatBehaviourClass behaviour_class(const char* name) {
    GoatBehaviourClass c{};
    c.name = name;
    c.create = [](GoatApi*, GoatEntity, const char* props) -> void* { return new T(props ? props : "null"); };
    c.start = [](void* s, GoatApi* g, GoatEntity e) { static_cast<T*>(s)->start(g, e); };
    c.update = [](void* s, GoatApi* g, GoatEntity e) { static_cast<T*>(s)->update(g, e); };
    c.free_state = [](void* s) { delete static_cast<T*>(s); };
    return c;
}

}  // namespace goat

// Circles around the origin at `radius` (props: {"radius": R}), facing its motion.
struct Orbit : goat::Behaviour {
    float radius = 3.0f;
    float angle = 0.0f;
    explicit Orbit(const std::string& props) {
        auto at = props.find("\"radius\"");
        if (at != std::string::npos) radius = std::stof(props.substr(props.find(':', at) + 1));
    }
    void update(GoatApi* g, GoatEntity self) override {
        angle += goat_delta_time(g);  // 1 rad/s
        GoatVec3 p{std::cos(angle) * radius, 0.0f, std::sin(angle) * radius};
        goat_set_position(g, self, p);
        goat_set_euler(g, self, GoatVec3{0.0f, -angle * 57.29578f, 0.0f});
    }
};

static const GoatBehaviourClass ORBIT = goat::behaviour_class<Orbit>("Orbit");

extern "C" void goat_example_cpp_init(void) { goat_register_behaviour(&ORBIT); }
