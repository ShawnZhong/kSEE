# The kSEE LLVM pass plugin, built against the LLVM that builds the kernel.
LLVM ?= -23

ksee-trace.so: ksee-trace.cpp
	clang++$(LLVM) -shared -fPIC -O2 $$(llvm-config$(LLVM) --cxxflags) $< -o $@

clean:
	rm -f ksee-trace.so
