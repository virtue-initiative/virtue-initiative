using Virtue.WindowsApp.Core.Interop;
using Xunit;

namespace Virtue.WindowsApp.Tests;

public sealed class StoreUpdateRetryPolicyTests
{
    [Fact]
    public void NoFailures_UsesSuccessInterval()
    {
        Assert.Equal(StoreUpdateRetryPolicy.SuccessInterval, StoreUpdateRetryPolicy.GetNextDelay(0));
    }

    [Fact]
    public void FirstFailure_UsesFirstRetry()
    {
        Assert.Equal(StoreUpdateRetryPolicy.FirstRetry, StoreUpdateRetryPolicy.GetNextDelay(1));
    }

    [Theory]
    [InlineData(1, 5)]
    [InlineData(2, 10)]
    [InlineData(3, 20)]
    [InlineData(4, 40)]
    public void EachFailure_DoublesThePreviousRetry(int consecutiveFailures, int expectedMinutes)
    {
        Assert.Equal(
            TimeSpan.FromMinutes(expectedMinutes),
            StoreUpdateRetryPolicy.GetNextDelay(consecutiveFailures));
    }

    [Fact]
    public void RetryDelay_GrowsMonotonicallyUpToTheCap()
    {
        var previous = StoreUpdateRetryPolicy.GetNextDelay(1);
        for (var failures = 2; failures <= 20; failures++)
        {
            var current = StoreUpdateRetryPolicy.GetNextDelay(failures);
            Assert.True(current >= previous, $"delay shrank at {failures} failures");
            Assert.True(current <= StoreUpdateRetryPolicy.MaxRetry, $"delay exceeded the cap at {failures} failures");
            previous = current;
        }
    }

    [Fact]
    public void RetryDelay_IsCappedAtMaxRetry()
    {
        Assert.Equal(StoreUpdateRetryPolicy.MaxRetry, StoreUpdateRetryPolicy.GetNextDelay(50));
        Assert.Equal(StoreUpdateRetryPolicy.MaxRetry, StoreUpdateRetryPolicy.GetNextDelay(int.MaxValue));
    }

    [Theory]
    [InlineData(-1)]
    [InlineData(int.MinValue)]
    public void NegativeFailureCount_IsTreatedAsNoFailures(int consecutiveFailures)
    {
        Assert.Equal(StoreUpdateRetryPolicy.SuccessInterval, StoreUpdateRetryPolicy.GetNextDelay(consecutiveFailures));
    }
}
