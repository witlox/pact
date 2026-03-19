Feature: Hardware Detection
  pact-agent detects detailed hardware capabilities (CPU, memory, network, storage)
  and includes them in the CapabilityReport. Each hardware category uses a backend
  trait with Linux and Mock implementations, following the GpuBackend pattern.

  Background:
    Given a journal with default state

  # --- CPU detection ---

  Scenario: Detect x86_64 CPU with AVX-512
    Given a node with an x86_64 CPU
      | field            | value                  |
      | model            | Intel Xeon w9-3495X    |
      | physical_cores   | 56                     |
      | logical_cores    | 112                    |
      | base_freq_mhz    | 1900                   |
      | max_freq_mhz     | 4800                   |
      | features         | avx512f,avx512bw,amx   |
      | numa_nodes       | 2                      |
      | cache_l3_bytes   | 105906176              |
    When capability detection runs
    Then the CPU architecture should be "X86_64"
    And the CPU model should be "Intel Xeon w9-3495X"
    And the CPU features should include "avx512f"
    And the CPU features should include "amx"

  Scenario: Detect aarch64 CPU with SVE
    Given a node with an aarch64 CPU
      | field            | value               |
      | model            | NVIDIA Grace         |
      | physical_cores   | 72                   |
      | logical_cores    | 72                   |
      | base_freq_mhz    | 3400                 |
      | max_freq_mhz     | 3400                 |
      | features         | sve,sve2,bf16        |
      | numa_nodes       | 1                    |
      | cache_l3_bytes   | 117440512            |
    When capability detection runs
    Then the CPU architecture should be "Aarch64"
    And the CPU features should include "sve"

  Scenario: Detect multi-socket system
    Given a node with an x86_64 CPU
      | field            | value                  |
      | model            | AMD EPYC 9654         |
      | physical_cores   | 192                    |
      | logical_cores    | 384                    |
      | base_freq_mhz    | 2400                   |
      | max_freq_mhz     | 3700                   |
      | features         | avx512f,avx512vnni     |
      | numa_nodes       | 4                      |
      | cache_l3_bytes   | 402653184              |
    When capability detection runs
    Then the CPU physical cores should be 192
    And the CPU logical cores should be 384
    And the CPU NUMA nodes should be 4

  Scenario: Detect core count with SMT enabled
    Given a node with an x86_64 CPU
      | field            | value               |
      | model            | Intel Xeon 8480+     |
      | physical_cores   | 56                   |
      | logical_cores    | 112                  |
      | base_freq_mhz    | 2000                 |
      | max_freq_mhz     | 3800                 |
      | features         | avx512f              |
      | numa_nodes       | 2                    |
      | cache_l3_bytes   | 107374182            |
    When capability detection runs
    Then the CPU physical cores should be 56
    And the CPU logical cores should be 112

  Scenario: Detect CPU frequency
    Given a node with an x86_64 CPU
      | field            | value               |
      | model            | Intel Xeon 8480+     |
      | physical_cores   | 56                   |
      | logical_cores    | 112                  |
      | base_freq_mhz    | 2000                 |
      | max_freq_mhz     | 3800                 |
      | features         | avx512f              |
      | numa_nodes       | 2                    |
      | cache_l3_bytes   | 107374182            |
    When capability detection runs
    Then the CPU base frequency should be 2000 MHz
    And the CPU max frequency should be 3800 MHz

  Scenario: CPU detection fails gracefully
    Given a node with no CPU info available
    When capability detection runs
    Then the CPU architecture should be "Unknown"
    And the CPU physical cores should be 0
    And the CPU logical cores should be 0

  # --- Memory detection ---

  Scenario: Detect total and available memory
    Given a node with memory
      | field            | value         |
      | total_bytes      | 549755813888  |
      | available_bytes  | 524288000000  |
      | memory_type      | DDR5          |
      | numa_nodes       | 2             |
    When capability detection runs
    Then the memory total should be 549755813888 bytes
    And the memory available should be 524288000000 bytes

  Scenario: Detect NUMA topology
    Given a node with 4 NUMA nodes
      | node_id | total_bytes    | cpus        |
      | 0       | 137438953472   | 0-27,112-139 |
      | 1       | 137438953472   | 28-55,140-167 |
      | 2       | 137438953472   | 56-83,168-195 |
      | 3       | 137438953472   | 84-111,196-223 |
    When capability detection runs
    Then the memory NUMA node count should be 4
    And NUMA node 0 should have 137438953472 total bytes
    And NUMA node 0 should contain CPU 0

  Scenario: Detect 2MB huge page allocation
    Given a node with huge pages
      | field           | value |
      | size_2mb_total  | 1024  |
      | size_2mb_free   | 512   |
      | size_1gb_total  | 0     |
      | size_1gb_free   | 0     |
    When capability detection runs
    Then the 2MB huge pages total should be 1024
    And the 2MB huge pages free should be 512

  Scenario: Detect 1GB huge page allocation
    Given a node with huge pages
      | field           | value |
      | size_2mb_total  | 0     |
      | size_2mb_free   | 0     |
      | size_1gb_total  | 64    |
      | size_1gb_free   | 32    |
    When capability detection runs
    Then the 1GB huge pages total should be 64
    And the 1GB huge pages free should be 32

  Scenario: Detect HBM memory type on unified memory node
    Given a node with memory
      | field            | value         |
      | total_bytes      | 917504000000  |
      | available_bytes  | 900000000000  |
      | memory_type      | HBM3e         |
      | numa_nodes       | 1             |
    When capability detection runs
    Then the memory type should be "HBM3e"

  Scenario: No NUMA info falls back to single node
    Given a node with memory but no NUMA topology
      | field            | value         |
      | total_bytes      | 274877906944  |
      | available_bytes  | 260000000000  |
      | memory_type      | DDR5          |
    When capability detection runs
    Then the memory NUMA node count should be 1
    And NUMA node 0 should have 274877906944 total bytes

  # --- Network detection ---

  Scenario: Detect Slingshot NICs
    Given a node with network interfaces
      | name  | driver | speed_mbps | state | mac               | ipv4         |
      | cxi0  | cxi    | 200000     | up    | 02:00:00:00:00:01 | 10.0.0.1     |
      | cxi1  | cxi    | 200000     | up    | 02:00:00:00:00:02 |              |
    When capability detection runs
    Then the network interface count should be 2
    And interface "cxi0" should have fabric "Slingshot"
    And interface "cxi0" should have speed 200000 Mbps

  Scenario: Detect Ethernet interfaces
    Given a node with network interfaces
      | name  | driver     | speed_mbps | state | mac               | ipv4        |
      | eth0  | i40e       | 1000       | up    | 00:1a:2b:3c:4d:5e | 10.1.0.5    |
    When capability detection runs
    Then interface "eth0" should have fabric "Ethernet"
    And interface "eth0" should have speed 1000 Mbps

  Scenario: Detect interface speed from sysfs
    Given a node with network interfaces
      | name  | driver | speed_mbps | state | mac               | ipv4      |
      | cxi0  | cxi    | 200000     | up    | 02:00:00:00:00:01 | 10.0.0.1  |
      | eth0  | i40e   | 1000       | up    | 00:1a:2b:3c:4d:5e | 10.1.0.5  |
    When capability detection runs
    Then interface "cxi0" should have speed 200000 Mbps
    And interface "eth0" should have speed 1000 Mbps

  Scenario: Detect link state
    Given a node with network interfaces
      | name  | driver | speed_mbps | state | mac               | ipv4      |
      | cxi0  | cxi    | 200000     | up    | 02:00:00:00:00:01 | 10.0.0.1  |
      | cxi1  | cxi    | 0          | down  | 02:00:00:00:00:02 |           |
    When capability detection runs
    Then interface "cxi0" should have state "Up"
    And interface "cxi1" should have state "Down"

  Scenario: No network interfaces returns empty list
    Given a node with no network interfaces
    When capability detection runs
    Then the network interface count should be 0

  # --- Storage detection ---

  Scenario: Detect diskless node
    Given a node with no local disks
    When capability detection runs
    Then the storage node type should be "Diskless"
    And the local disk count should be 0

  Scenario: Detect NVMe local storage
    Given a node with local disks
      | device      | model              | capacity_bytes   | disk_type |
      | /dev/nvme0n1 | Samsung PM9A3     | 3840755604480    | Nvme      |
      | /dev/nvme1n1 | Samsung PM9A3     | 3840755604480    | Nvme      |
    When capability detection runs
    Then the storage node type should be "LocalStorage"
    And the local disk count should be 2
    And disk "/dev/nvme0n1" should have type "Nvme"
    And disk "/dev/nvme0n1" should have capacity 3840755604480 bytes

  Scenario: Detect NFS mounts with capacity
    Given a node with mounts
      | path         | fs_type | source                   | total_bytes     | available_bytes  |
      | /home        | Nfs     | nfs-server:/export/home  | 107374182400000 | 53687091200000   |
      | /scratch     | Nfs     | nfs-server:/export/scratch | 214748364800000 | 107374182400000 |
    When capability detection runs
    Then mount "/home" should have fs type "Nfs"
    And mount "/home" should have total 107374182400000 bytes
    And mount "/home" should have available 53687091200000 bytes

  Scenario: Detect Lustre mounts
    Given a node with mounts
      | path         | fs_type | source        | total_bytes     | available_bytes  |
      | /lustre/work | Lustre  | lustre@o2ib:/ | 429496729600000 | 214748364800000  |
    When capability detection runs
    Then mount "/lustre/work" should have fs type "Lustre"

  Scenario: Detect tmpfs scratch with capacity
    Given a node with mounts
      | path     | fs_type | source | total_bytes  | available_bytes |
      | /tmp     | Tmpfs   | tmpfs  | 274877906944 | 274877906944    |
    When capability detection runs
    Then mount "/tmp" should have fs type "Tmpfs"
    And mount "/tmp" should have total 274877906944 bytes
    And mount "/tmp" should have available 274877906944 bytes

  Scenario: No mounts returns empty list
    Given a node with no mounts
    When capability detection runs
    Then the mount count should be 0

  # --- Software detection ---

  Scenario: Detect loaded kernel modules
    Given a node with loaded modules
      | module            |
      | cxi_core          |
      | nvidia            |
      | nfs               |
    When capability detection runs
    Then the loaded modules should include "cxi_core"
    And the loaded modules should include "nvidia"

  Scenario: Detect uenv image from overlay metadata
    Given a node with uenv image "ml-training:v2.3.1"
    When capability detection runs
    Then the uenv image should be "ml-training:v2.3.1"

  Scenario: Detect running services from supervisor
    Given 3 declared services with 2 running and 1 failed
    When capability detection runs
    Then the software services count should be 3
    And the supervisor status should show 3 declared, 2 running, 1 failed

  # --- Cross-category ---

  Scenario: Full capability report includes all categories
    Given a node with an x86_64 CPU
      | field            | value               |
      | model            | Intel Xeon 8480+     |
      | physical_cores   | 56                   |
      | logical_cores    | 112                  |
      | base_freq_mhz    | 2000                 |
      | max_freq_mhz     | 3800                 |
      | features         | avx512f              |
      | numa_nodes       | 2                    |
      | cache_l3_bytes   | 107374182            |
    And a node with 4 NVIDIA A100 GPUs
    And a node with memory
      | field            | value         |
      | total_bytes      | 549755813888  |
      | available_bytes  | 524288000000  |
      | memory_type      | DDR5          |
      | numa_nodes       | 2             |
    And a node with network interfaces
      | name  | driver | speed_mbps | state | mac               | ipv4      |
      | cxi0  | cxi    | 200000     | up    | 02:00:00:00:00:01 | 10.0.0.1  |
    And a node with no local disks
    When capability detection runs
    Then the capability report should include a CPU section
    And the capability report should contain 4 GPUs
    And the capability report should include a memory section
    And the network interface count should be 1
    And the capability report should include a storage section
    And the capability report should include a software section

  Scenario: Capability change triggers report update
    Given a stable capability report
    And a node with network interfaces
      | name  | driver | speed_mbps | state | mac               | ipv4      |
      | cxi0  | cxi    | 200000     | up    | 02:00:00:00:00:01 | 10.0.0.1  |
    When interface "cxi0" transitions from up to down
    Then a new capability report should be sent immediately
