// A C# behaviour: breathes its object's scale; the X button makes it pop (counted in PlayerPrefs).
// tools/goat-cs translates it to Pulse.cs.js (the editor does it when the file changes).
using GoatEngine;

public class Pulse : MonoBehaviour
{
    public float amount = 0.08f;
    public float speed = 1.5f;

    Vector3 baseScale;
    float t;
    float pop;

    void Start()
    {
        baseScale = transform.localScale;
    }

    void Update()
    {
        t += Time.deltaTime;
        if (Input.GetButtonDown("X"))
        {
            pop = 1f;
            // remembered between runs (PlayerPrefs: AppData on desktop, SD card on the 3DS)
            var pops = PlayerPrefs.GetInt("pops") + 1;
            PlayerPrefs.SetInt("pops", pops);
            Debug.Log($"pop #{pops}");
        }
        pop = Mathf.MoveTowards(pop, 0f, Time.deltaTime * 3f);
        var s = 1f + Mathf.Sin(t * speed * 2f * Mathf.PI) * amount + pop * 0.35f;
        transform.localScale = baseScale * s;
    }
}
