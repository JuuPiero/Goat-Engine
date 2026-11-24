using GoatEngine;
using System.Collections.Generic;

namespace Game
{
    public enum Mood { Calm, Excited = 5, Sleepy }

    /// Floats up and down, spins, counts frames; A spawns a coin, B removes them.
    public class Hover : MonoBehaviour
    {
        public float height = 1.5f;
        public float speed = 2f;
        public Vector3 offset = new Vector3(0, 1, 0);
        public Mood mood = Mood.Excited;
        int frames;
        float t;
        readonly List<GameObject> coins = new List<GameObject>();
        public int Frames => frames;

        void Start()
        {
            transform.position = offset;
            Debug.Log($"hover start on {name}, height {height:F1}, mood {mood}");
        }

        void Update()
        {
            frames++;
            t += Time.deltaTime;
            var p = transform.position;
            p.y = offset.y + Mathf.Sin(t * speed) * height;
            transform.position = p + Vector3.right * 0.5f;
            transform.Rotate(0, 90 * Time.deltaTime, 0);
            if (Input.GetButtonDown("A"))
            {
                var coin = GameObject.Create("Coin " + coins.Count);
                coin.transform.position = new Vector3(coins.Count, 3, 0);
                coin.SetComponent("MeshRenderer", new { mesh = "builtin:sphere" });
                coins.Add(coin);
            }
            if (Input.GetButtonDown("B"))
            {
                foreach (var c in coins) GameObject.Destroy(c);
                coins.Clear();
                Debug.LogWarning("coins cleared at frame", frames / 2);
            }
            var doubled = gameObject.Call("Test.double", 21);
            if (doubled != 42.0) Debug.LogError("extension call returned", doubled);
        }
    }
}
