# seL4 kernel config for bare-metal x86_64 boot-time measurement.
# Used via `cmake -C`. Not the verified configuration (printing + debug on).
set(KernelPlatform "pc99" CACHE STRING "")
set(KernelSel4Arch "x86_64" CACHE STRING "")
set(KernelVerificationBuild OFF CACHE BOOL "")
set(KernelDebugBuild ON CACHE BOOL "")
set(KernelPrinting ON CACHE BOOL "")
set(KernelOptimisation "-O2" CACHE STRING "")
set(KernelMaxNumNodes "1" CACHE STRING "")
set(KernelRootCNodeSizeBits "16" CACHE STRING "")
# Vendor DMAR tables are a common cause of early panics; not needed here.
set(KernelIOMMU OFF CACHE BOOL "")
# Portable across QEMU (Nehalem model) and Kaby/Coffee Lake NUCs.
set(KernelFSGSBase "msr" CACHE STRING "")
set(KernelSupportPCID OFF CACHE BOOL "")
set(KernelFPU "FXSAVE" CACHE STRING "")
# 0 = calibrate against the PIT (~100 ms). Set to the host TSC frequency in
# Hz to skip calibration; passed by build-baremetal.sh as -DKernelPC99TSCFrequency.
set(KernelPC99TSCFrequency "0" CACHE STRING "")
