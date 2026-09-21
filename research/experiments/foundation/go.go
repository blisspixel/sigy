package main

import (
	"fmt"
	"sync"
	"time"
)

func main() {
	var mutex sync.Mutex
	available := uint64(10000)
	accepted := uint64(0)
	var workers sync.WaitGroup
	for range 16 {
		workers.Go(func() {
			for range 1000 {
				mutex.Lock()
				if available >= 13 {
					available -= 13
					accepted++
				}
				mutex.Unlock()
			}
		})
	}
	workers.Wait()
	if accepted != 769 || available != 3 {
		panic("budget invariant failed")
	}

	started := time.Now()
	blocks := make(chan []byte, 64)
	type result struct{ sum, count uint64 }
	done := make(chan result, 1)
	go func() {
		var observed result
		for block := range blocks {
			for _, value := range block {
				observed.sum += uint64(value)
			}
			observed.count++
		}
		done <- observed
	}()
	var expected uint64
	for i := range 20000 {
		value := byte(i % 251)
		block := make([]byte, 4096)
		for j := range block {
			block[j] = value
		}
		expected += uint64(value) * 4096
		blocks <- block
	}
	close(blocks)
	if observed := <-done; observed.sum != expected || observed.count != 20000 {
		panic("queue integrity failed")
	}
	elapsed := time.Since(started).Microseconds()

	blocked := make(chan byte, 1)
	blocked <- 1
	cancel := make(chan struct{})
	cancelled := make(chan bool, 1)
	go func() {
		select {
		case blocked <- 2:
			cancelled <- false
		case <-cancel:
			cancelled <- true
		}
	}()
	close(cancel)
	if !<-cancelled {
		panic("blocked producer did not cancel")
	}
	fmt.Printf("accepted=%d remaining=3 blocks=20000 bytes=81920000 queue_us=%d cancellation=pass\n", accepted, elapsed)
}
