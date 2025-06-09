PY_VERSIONS = 3.8 3.9 3.10 3.11 3.12

all: build

build:
	@for py in $(PY_VERSIONS); do \
		echo "Building for Python $$py..."; \
		maturin build --release --interpreter python$$py; \
	done

clean:
	rm -rf target/wheels/*

upload:
	twine upload target/wheels/*
