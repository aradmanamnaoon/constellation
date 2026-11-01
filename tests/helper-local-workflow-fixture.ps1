# In-process, loopback-only OpenAI Chat Completions response fixture. No model
# inference, paid API, external listener or credentials. Used only for the
# explicit -TestWorkflowStep opt-in within the hidden-desktop UIA worker.
if (-not ('ConstellationLocalWorkflowFixture' -as [type])) {
  $localFixtureSource = @'
using System;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using System.IO;

public sealed class ConstellationLocalWorkflowFixture : IDisposable {
  public const string Model = "workflow-local-fixture-no-model";
  public const string Result = "CONSTELLATION_LOCALHOST_WORKFLOW_E2E_SUCCESS";
  private readonly TcpListener listener;
  private readonly Thread worker;
  private int stopped, requestCount, rejectedCount;
  public int Port { get; private set; }
  public int RequestCount { get { return Interlocked.CompareExchange(ref requestCount, 0, 0); } }
  public int RejectedCount { get { return Interlocked.CompareExchange(ref rejectedCount, 0, 0); } }
  public string Url { get { return "http://127.0.0.1:" + Port + "/v1"; } }
  public ConstellationLocalWorkflowFixture() {
    listener = new TcpListener(IPAddress.Loopback, 0);
    listener.Start(2);
    Port = ((IPEndPoint)listener.LocalEndpoint).Port;
    worker = new Thread(Serve) { IsBackground = true, Name = "Constellation local workflow fixture" };
    worker.Start();
  }
  private void Serve() {
    while (Interlocked.CompareExchange(ref stopped, 0, 0) == 0) {
      try {
        using (TcpClient client = listener.AcceptTcpClient()) {
          client.ReceiveTimeout = 5000;
          client.SendTimeout = 5000;
          using (NetworkStream stream = client.GetStream()) {
            byte[] bytes = new byte[65536];
            int total = 0, bodyStart = -1;
            while (total < bytes.Length) {
              int count = stream.Read(bytes, total, bytes.Length - total);
              if (count <= 0) break;
              total += count;
              bodyStart = FindHeadersEnd(bytes, total);
              if (bodyStart >= 0) break;
            }
            string header = bodyStart >= 0 ? Encoding.ASCII.GetString(bytes, 0, bodyStart) : "";
            int size = -1;
            foreach (string line in header.Split(new string[] { "\r\n" }, StringSplitOptions.None)) {
              if (line.StartsWith("Content-Length:", StringComparison.OrdinalIgnoreCase))
                int.TryParse(line.Substring(15).Trim(), out size);
            }
            bool route = header.StartsWith("POST /v1/chat/completions HTTP/1.1\r\n", StringComparison.Ordinal);
            bool bounded = size >= 1 && size <= 32768 && bodyStart >= 0 && bodyStart + size <= bytes.Length;
            if (bounded) {
              while (total - bodyStart < size) {
                int count = stream.Read(bytes, total, bytes.Length - total);
                if (count <= 0) break;
                total += count;
              }
            }
            string body = bounded && total - bodyStart >= size
              ? Encoding.UTF8.GetString(bytes, bodyStart, size) : "";
            bool valid = route && bounded && body.Contains("\"model\"")
              && body.Contains(Model) && body.Contains("\"messages\"");
            if (valid) Interlocked.Increment(ref requestCount);
            else Interlocked.Increment(ref rejectedCount);
            string json = valid
              ? "{\"id\":\"fixture-local\",\"object\":\"chat.completion\",\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\",\"content\":\""+ Result +"\"},\"finish_reason\":\"stop\"}]}"
              : "{\"error\":\"Unexpected localhost request\"}";
            byte[] payload = Encoding.UTF8.GetBytes(json);
            string status = valid ? "200 OK" : "400 Bad Request";
            byte[] prefix = Encoding.ASCII.GetBytes("HTTP/1.1 "+status+"\r\nContent-Type: application/json\r\nContent-Length: " + payload.Length + "\r\nConnection: close\r\n\r\n");
            stream.Write(prefix, 0, prefix.Length);
            stream.Write(payload, 0, payload.Length);
          }
        }
      } catch (Exception) {
        if (Interlocked.CompareExchange(ref stopped, 0, 0) != 0) break;
        Interlocked.Increment(ref rejectedCount);
      }
    }
  }
  private static int FindHeadersEnd(byte[] data, int length) {
    for (int i = 3; i < length; i++)
      if (data[i-3] == 13 && data[i-2] == 10 && data[i-1] == 13 && data[i] == 10) return i+1;
    return -1;
  }
  public void Dispose() {
    Interlocked.Exchange(ref stopped, 1);
    listener.Stop();
    worker.Join(2500);
  }
}
'@
  Add-Type -TypeDefinition $localFixtureSource -ReferencedAssemblies 'System' -ErrorAction Stop
}
