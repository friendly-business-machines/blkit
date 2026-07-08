package awssqssns_test

import (
	"context"
	"crypto/rand"
	"fmt"
	"os"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/aws/aws-sdk-go-v2/credentials"
	"github.com/testcontainers/testcontainers-go"
	"github.com/testcontainers/testcontainers-go/modules/localstack"

	awsbroker "github.com/friendly-business-machines/blkit/brokers/aws-sqs-sns"
	bl "github.com/friendly-business-machines/blkit/core"
)

// The suite runs against SQS + SNS + DynamoDB. By default it starts ONE
// LocalStack container per test binary via testcontainers-go; set
// BLKIT_TEST_AWS_ENDPOINT to point it at an already-running LocalStack (or a
// real account endpoint) instead. It skips only when neither an endpoint nor
// a reachable Docker daemon is available. Each open gets a fresh queue prefix
// and a fresh DynamoDB table so subtests are isolated and repeatable.
func TestAWSSQSSNSMessageBrokerConformance(t *testing.T) {
	bl.RunMessageBrokerConformance(t, func(t *testing.T) bl.MessageBroker {
		return openBroker(t, nil)
	}, bl.BrokerConformanceOptions{
		SupportsQueueRemoval: false, // SQS cannot remove queued messages; Cancel always routes a JobCancel
		RegistrationTTL:      1500 * time.Millisecond,
		InFlightTimeout:      time.Second,
		OpenWithCipher: func(t *testing.T) bl.MessageBroker {
			return openBroker(t, testCipher(t))
		},
	})
}

var openCount atomic.Int64

// openBroker returns a fresh Broker (and its DynamoRegistryStore) on isolated
// infrastructure: a unique queue prefix and DynamoDB table per call.
func openBroker(t *testing.T, cipher bl.PayloadCipher) bl.MessageBroker {
	t.Helper()
	endpoint := awsEndpoint(t)
	region := os.Getenv("AWS_REGION")
	if region == "" {
		region = "us-east-1"
	}
	creds := credentials.NewStaticCredentialsProvider("test", "test", "")
	suffix := fmt.Sprintf("%d-%d", time.Now().UnixNano()%1_000_000_000, openCount.Add(1))

	store, err := awsbroker.NewDynamoRegistryStore(awsbroker.DynamoRegistryStoreConfig{
		Region:      region,
		Credentials: creds,
		Endpoint:    endpoint,
		TableName:   "blkit-reg-" + suffix,
		Cipher:      cipher,
	})
	if err != nil {
		t.Fatalf("registry store: %v", err)
	}
	t.Cleanup(func() { _ = store.Close() })

	broker, err := awsbroker.New(awsbroker.Config{
		Region:          region,
		Credentials:     creds,
		QueuePrefix:     "blkit-" + suffix,
		Registry:        store,
		Endpoint:        endpoint,
		Cipher:          cipher,
		RegistrationTTL: 1500 * time.Millisecond,
		InFlightTimeout: time.Second,
		EventRetention:  time.Hour,
	})
	if err != nil {
		t.Fatalf("broker: %v", err)
	}
	return broker
}

func testCipher(t *testing.T) bl.PayloadCipher {
	t.Helper()
	key := make([]byte, 32)
	if _, err := rand.Read(key); err != nil {
		t.Fatalf("cipher key: %v", err)
	}
	cipher, err := bl.NewAESGCMPayloadCipher("conformance-key", key)
	if err != nil {
		t.Fatalf("cipher: %v", err)
	}
	return cipher
}

var (
	localstackOnce     sync.Once
	localstackEndpoint string
	localstackErr      error
)

// awsEndpoint returns the AWS endpoint override for the suite. It prefers
// BLKIT_TEST_AWS_ENDPOINT; when that is unset it starts one LocalStack
// container for the whole test binary (reaped by testcontainers' Ryuk).
func awsEndpoint(t *testing.T) string {
	t.Helper()
	if ep := os.Getenv("BLKIT_TEST_AWS_ENDPOINT"); ep != "" {
		return ep
	}
	localstackOnce.Do(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Minute)
		defer cancel()
		ctr, err := localstack.Run(ctx, "localstack/localstack:4.0",
			testcontainers.WithEnv(map[string]string{"SERVICES": "sqs,sns,dynamodb"}),
		)
		if err != nil {
			localstackErr = err
			return
		}
		localstackEndpoint, localstackErr = ctr.PortEndpoint(ctx, "4566/tcp", "http")
	})
	if localstackErr != nil {
		t.Skipf("start localstack container (set BLKIT_TEST_AWS_ENDPOINT to use an existing endpoint): %v", localstackErr)
	}
	return localstackEndpoint
}
