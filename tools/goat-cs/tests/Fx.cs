using GoatEngine;
using System.Collections;

public class Fx : MonoBehaviour
{
    public float value;
    int stage;

    void Start()
    {
        StartCoroutine(Blink());
        transform.DOMove(new Vector3(4f, 0f, 0f), 0.4f).SetEase(Ease.Linear).SetLoops(2, LoopType.Yoyo).OnComplete(() => stage = 9);
        DOTween.To(() => value, v => value = v, 10f, 0.2f);
        DOTween.Sequence().AppendInterval(0.3f).AppendCallback(() => Debug.Log("seq"));
        Time.timeScale = 1f;
        Wait();
    }

    IEnumerator Blink()
    {
        stage = 1;
        yield return new WaitForSeconds(0.2f);
        stage = 2;
        yield return null;
        if (stage > 5) yield break;
        yield return new WaitUntil(() => Time.time > 0.45f);
        stage = 3;
    }

    async void Wait()
    {
        await Awaitable.WaitForSecondsAsync(0.25f);
        Debug.Log("awaited");
    }
}
