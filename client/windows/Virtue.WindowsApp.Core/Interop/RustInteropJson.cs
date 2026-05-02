using System.Text.Json;

namespace Virtue.WindowsApp.Core.Interop;

public static class RustInteropJson
{
    public static readonly JsonSerializerOptions SerializerOptions = new()
    {
        PropertyNameCaseInsensitive = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        WriteIndented = false,
    };

    public static T DeserializePayload<T>(string raw)
    {
        try
        {
            var value = JsonSerializer.Deserialize<T>(raw, SerializerOptions);
            return value ?? throw new InvalidOperationException("Interop payload was empty.");
        }
        catch (JsonException ex)
        {
            throw new InvalidOperationException($"Interop call returned a non-JSON payload: {raw}", ex);
        }
    }

    public static string Serialize<T>(T value) => JsonSerializer.Serialize(value, SerializerOptions);
}
